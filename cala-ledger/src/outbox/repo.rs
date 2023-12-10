use sqlx::{PgPool, Postgres, QueryBuilder, Row, Transaction};

use super::{error::*, event::*};

#[derive(Clone)]
pub(super) struct OutboxRepo {
    _pool: PgPool,
}

impl OutboxRepo {
    pub(super) fn new(pool: &PgPool) -> Self {
        Self {
            _pool: pool.clone(),
        }
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
}
