use es_entity::*;
use sqlx::PgPool;

use crate::outbox::OutboxPublisher;

use super::entity::*;

#[derive(EsRepo, Clone)]
#[es_repo(
    entity = "TxTemplate",
    columns(code(
        ty = "String",
        update(accessor = "values().code", persist = false),
        list_by
    ),),
    tbl_prefix = "cala",
    post_persist_hook = "publish",
    persist_event_context = false
)]
pub(super) struct TxTemplateRepo {
    pool: PgPool,
    publisher: OutboxPublisher,
}

impl TxTemplateRepo {
    pub fn new(pool: &PgPool, publisher: &OutboxPublisher) -> Self {
        Self {
            pool: pool.clone(),
            publisher: publisher.clone(),
        }
    }

    async fn publish(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        entity: &TxTemplate,
        new_events: es_entity::LastPersisted<'_, TxTemplateEvent>,
    ) -> Result<(), sqlx::Error> {
        self.publisher
            .publish_entity_events(op, entity, new_events)
            .await?;
        Ok(())
    }
}
