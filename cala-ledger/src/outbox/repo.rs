use chrono::{DateTime, Utc};
use sqlx::{PgPool, Postgres, QueryBuilder, Row, Transaction};

use super::{error::*, event::*};

#[derive(Clone)]
pub(super) struct OutboxRepo {
    pool: PgPool,
}

impl OutboxRepo {
    pub(super) fn new(pool: &PgPool) -> Self {
        Self { pool: pool.clone() }
    }

    pub async fn highest_known_sequence(&self) -> Result<EventSequence, OutboxError> {
        let row =
            sqlx::query!(r#"SELECT COALESCE(MAX(sequence), 0) AS "max!" FROM cala_outbox_events"#)
                .fetch_one(&self.pool)
                .await?;
        Ok(EventSequence::from(row.max as u64))
    }

    pub async fn persist_events(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        recorded_at: Option<DateTime<Utc>>,
        events: impl Iterator<Item = OutboxEventPayload>,
    ) -> Result<Vec<OutboxEvent>, OutboxError> {
        let mut query_builder: QueryBuilder<Postgres> = QueryBuilder::new(format!(
            "WITH new_events AS (INSERT INTO cala_outbox_events (payload{})",
            recorded_at.map(|_| ", recorded_at").unwrap_or("")
        ));
        let mut payloads = Vec::new();
        query_builder.push_values(events, |mut builder, payload| {
            builder.push_bind(serde_json::to_value(&payload).expect("Could not serialize payload"));
            if let Some(recorded_at) = recorded_at {
                builder.push_bind(recorded_at);
            }
            payloads.push(payload);
        });
        query_builder.push(
            r#"RETURNING id, sequence, recorded_at )
               SELECT * FROM new_events
               ORDER BY sequence"#,
        );
        let query = query_builder.build();
        let rows = query.fetch_all(&mut **tx).await?;
        let events = rows
            .into_iter()
            .zip(payloads.into_iter())
            .map(|(row, payload)| OutboxEvent {
                id: row.get::<OutboxEventId, _>("id"),
                sequence: row.get("sequence"),
                recorded_at: row.get("recorded_at"),
                payload,
            })
            .collect::<Vec<_>>();
        Ok(events)
    }

    pub async fn load_next_page(
        &self,
        from_sequence: EventSequence,
        buffer_size: usize,
    ) -> Result<Vec<OutboxEvent>, OutboxError> {
        let rows = sqlx::query!(
            r#"
            WITH max_sequence AS (
                SELECT COALESCE(MAX(sequence), 0) AS max FROM cala_outbox_events
            )
            SELECT
              g.seq AS "sequence!: EventSequence",
              e.id AS "id?",
              e.payload AS "payload?",
              e.recorded_at AS "recorded_at?"
            FROM
                generate_series(LEAST($1 + 1, (SELECT max FROM max_sequence)),
                  LEAST($1 + $2, (SELECT max FROM max_sequence)))
                AS g(seq)
            LEFT JOIN
                cala_outbox_events e ON g.seq = e.sequence
            WHERE
                g.seq > $1
            ORDER BY
                g.seq ASC
            LIMIT $2"#,
            from_sequence as EventSequence,
            buffer_size as i64,
        )
        .fetch_all(&self.pool)
        .await?;
        let mut events = Vec::new();
        let mut empty_ids = Vec::new();
        for row in rows {
            if row.id.is_none() {
                empty_ids.push(row.sequence);
                continue;
            }
            events.push(OutboxEvent {
                id: OutboxEventId::from(row.id.expect("already checked")),
                sequence: row.sequence,
                payload: row
                    .payload
                    .map(|p| serde_json::from_value(p).expect("Could not deserialize payload"))
                    .unwrap_or(OutboxEventPayload::Empty),
                recorded_at: row.recorded_at.unwrap_or_default(),
            });
        }

        if !empty_ids.is_empty() {
            let rows = sqlx::query!(
                r#"
                INSERT INTO cala_outbox_events (sequence)
                SELECT unnest($1::bigint[]) AS sequence
                ON CONFLICT (sequence) DO UPDATE
                SET sequence = EXCLUDED.sequence
                RETURNING id, sequence AS "sequence!: EventSequence", payload, recorded_at
            "#,
                &empty_ids as &[EventSequence]
            )
            .fetch_all(&self.pool)
            .await?;
            for row in rows {
                events.push(OutboxEvent {
                    id: OutboxEventId::from(row.id),
                    sequence: row.sequence,
                    payload: row
                        .payload
                        .map(|p| serde_json::from_value(p).expect("Could not deserialize payload"))
                        .unwrap_or(OutboxEventPayload::Empty),
                    recorded_at: row.recorded_at,
                });
            }
            events.sort_by(|a, b| a.sequence.cmp(&b.sequence));
        }

        Ok(events)
    }
}
