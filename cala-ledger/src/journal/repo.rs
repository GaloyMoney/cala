use es_entity::*;
use sqlx::PgPool;

use crate::outbox::OutboxPublisher;

use super::{entity::*, error::JournalError};

#[derive(EsRepo, Debug, Clone)]
#[es_repo(
    entity = "Journal",
    err = "JournalError",
    columns(
        name(ty = "String", update(accessor = "values().name")),
        code(ty = "Option<String>", update(accessor = "values().code")),
    ),
    tbl_prefix = "cala",
    post_persist_hook = "publish",
    persist_event_context = false
)]
pub(super) struct JournalRepo {
    pool: PgPool,
    publisher: OutboxPublisher,
}

impl JournalRepo {
    pub fn new(pool: &PgPool, publisher: &OutboxPublisher) -> Self {
        Self {
            pool: pool.clone(),
            publisher: publisher.clone(),
        }
    }

    async fn publish(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        entity: &Journal,
        new_events: es_entity::LastPersisted<'_, JournalEvent>,
    ) -> Result<(), JournalError> {
        self.publisher
            .publish_entity_events(op, entity, new_events)
            .await?;
        Ok(())
    }
}
