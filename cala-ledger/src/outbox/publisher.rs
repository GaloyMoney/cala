use cala_types::outbox::OutboxEventPayload;
use chrono::{DateTime, Utc};
use sqlx::{Postgres, Transaction};

use super::ObixOutbox;

#[derive(Debug, Clone)]
pub struct OutboxPublisher {
    outbox: ObixOutbox,
}

impl OutboxPublisher {
    pub async fn init(pool: &sqlx::PgPool) -> Result<Self, sqlx::Error> {
        let outbox = ObixOutbox::init(pool, Default::default()).await?;
        Ok(Self { outbox })
    }

    pub async fn publish_entity_events<Entity, Event>(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        _: Entity,
        new_events: es_entity::LastPersisted<'_, Event>,
    ) -> Result<(), sqlx::Error>
    where
        Event: es_entity::EsEvent,
        for<'a> &'a Event: Into<OutboxEventPayload>,
    {
        self.outbox
            .publish_all_persisted(op, new_events.map(|e| &e.event))
            .await
    }

    pub async fn publish_all(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        events: impl Iterator<Item = OutboxEventPayload>,
    ) -> Result<(), sqlx::Error> {
        self.outbox.publish_all_persisted(op, events).await
    }

    pub fn outbox(&self) -> &ObixOutbox {
        &self.outbox
    }

    #[cfg(feature = "import")]
    pub async fn persist_events_at(
        &self,
        db: impl Into<Transaction<'_, Postgres>>,
        events: impl IntoIterator<Item = impl Into<OutboxEventPayload>>,
        recorded_at: impl Into<Option<DateTime<Utc>>>,
    ) -> Result<(), sqlx::Error> {
        let mut db = db.into();
        let recorded_at = recorded_at.into();

        // If we have a specific recorded_at time, we need to use custom SQL
        // Otherwise, we can use obix's publish_all_persisted
        if let Some(recorded_at) = recorded_at {
            // Use custom SQL for recording at a specific time (import feature)
            use sqlx::QueryBuilder;
            let mut query_builder: QueryBuilder<Postgres> =
                QueryBuilder::new("INSERT INTO cala_outbox_events (payload, recorded_at)");
            query_builder.push_values(events, |mut builder, payload| {
                let payload = payload.into();
                builder.push_bind(
                    serde_json::to_value(&payload).expect("Could not serialize payload"),
                );
                builder.push_bind(recorded_at);
            });
            query_builder.build().execute(&mut *db).await?;
        } else {
            // Use obix for normal case
            let mut op = es_entity::DbOp::from(db);
            self.outbox
                .publish_all_persisted(&mut op, events.into_iter().map(Into::into))
                .await?;
            db = op.into();
        }

        db.commit().await?;
        Ok(())
    }
}
