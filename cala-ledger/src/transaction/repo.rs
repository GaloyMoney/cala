use es_entity::*;
use sqlx::PgPool;

use crate::{outbox::OutboxPublisher, primitives::{JournalId, TransactionId, TxTemplateId}};

use super::{entity::*, error::TransactionError};

#[derive(EsRepo, Clone)]
#[es_repo(
    entity = "Transaction",
    err = "TransactionError",
    columns(
        external_id(ty = "Option<String>", update(persist = false)),
        correlation_id(ty = "String", update(persist = false)),
        journal_id(ty = "JournalId", update(persist = false)),
        tx_template_id(ty = "TxTemplateId", update(persist = false), list_for),
        effective(ty = "chrono::NaiveDate", update(persist = false)),
    ),
    tbl_prefix = "cala",
    post_persist_hook = "publish",
    persist_event_context = false
)]
pub(super) struct TransactionRepo {
    pool: PgPool,
    publisher: OutboxPublisher,
}

impl TransactionRepo {
    pub fn new(pool: &PgPool, publisher: &OutboxPublisher) -> Self {
        Self {
            pool: pool.clone(),
            publisher: publisher.clone(),
        }
    }

    async fn publish(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        entity: &Transaction,
        new_events: es_entity::LastPersisted<'_, TransactionEvent>,
    ) -> Result<(), TransactionError> {
        self.publisher
            .publish_entity_events(op, entity, new_events)
            .await?;
        Ok(())
    }
}
