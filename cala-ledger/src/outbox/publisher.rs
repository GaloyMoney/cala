use cala_types::outbox::OutboxEventPayload;

#[derive(Debug, obix::MailboxTables)]
#[obix(tbl_prefix = "cala")]
struct CalaMailboxTables;

type Outbox = obix::Outbox<OutboxEventPayload, CalaMailboxTables>;

#[derive(Debug, Clone)]
pub struct OutboxPublisher {
    outbox: Outbox,
}

impl OutboxPublisher {
    pub async fn init(pool: &sqlx::PgPool) -> Result<Self, sqlx::Error> {
        let outbox = Outbox::init(pool, Default::default()).await?;
        Ok(Self { outbox })
    }

    pub async fn publish_all<Entity, Event>(
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
}
