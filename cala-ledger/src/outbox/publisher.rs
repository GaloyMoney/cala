use cala_types::outbox::OutboxEventPayload;

use super::ObixOutbox;

#[derive(Debug, Clone)]
pub struct OutboxPublisher {
    inner: ObixOutbox,
}

impl OutboxPublisher {
    pub async fn init(pool: &sqlx::PgPool) -> Result<Self, sqlx::Error> {
        let outbox = ObixOutbox::init(pool, Default::default()).await?;
        Ok(Self { inner: outbox })
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
        self.inner
            .publish_all_persisted(op, new_events.map(|e| &e.event))
            .await
    }

    pub async fn publish_all(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        events: impl Iterator<Item = OutboxEventPayload>,
    ) -> Result<(), sqlx::Error> {
        self.inner.publish_all_persisted(op, events).await
    }

    pub fn inner(&self) -> &ObixOutbox {
        &self.inner
    }
}
