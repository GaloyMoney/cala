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
            sqlx::query!(r#"SELECT COALESCE(MAX(sequence), 0) AS "max" FROM cala_outbox_events"#)
                .fetch_one(&self.pool)
                .await?;
        Ok(EventSequence::from(row.max.unwrap_or(0) as u64))
    }

    pub async fn persist_events(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        events: impl Iterator<Item = OutboxEventPayload>,
    ) -> Result<Vec<OutboxEvent>, OutboxError> {
        let mut query_builder: QueryBuilder<Postgres> = QueryBuilder::new(
            r#"WITH new_events AS (
               INSERT INTO cala_outbox_events (payload)"#,
        );
        let mut payloads = Vec::new();
        query_builder.push_values(events, |mut builder, payload| {
            builder.push_bind(serde_json::to_value(&payload).expect("Could not serialize payload"));
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
        sequence: EventSequence,
        buffer_size: usize,
    ) -> Result<Vec<OutboxEvent>, OutboxError> {
        let rows = sqlx::query!(
            r#"
            SELECT
              g.seq AS "sequence!: EventSequence",
              e.id,
              e.payload AS "payload?",
              e.recorded_at AS "recorded_at?"
            FROM
                generate_series($1 + 1, $1 + $2) AS g(seq)
            LEFT JOIN
                cala_outbox_events e ON g.seq = e.sequence
            WHERE
                g.seq > $1
            ORDER BY
                g.seq ASC
            LIMIT $2"#,
            sequence as EventSequence,
            buffer_size as i64,
        )
        .fetch_all(&self.pool)
        .await?;
        let mut events = Vec::new();
        for row in rows {
            events.push(OutboxEvent {
                id: OutboxEventId::from(row.id),
                sequence: row.sequence,
                payload: row
                    .payload
                    .map(|p| serde_json::from_value(p).expect("Could not deserialize payload"))
                    .unwrap_or(OutboxEventPayload::Empty),
                recorded_at: row.recorded_at.unwrap_or_default(),
            });
        }
        Ok(events)
    }
}
