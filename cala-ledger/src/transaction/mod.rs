pub mod error;

mod entity;
mod repo;

use sqlx::PgPool;
use tracing::instrument;

use std::collections::HashMap;

use crate::outbox::*;
use crate::primitives::TxTemplateId;

pub use entity::*;
use error::*;
pub use repo::transaction_cursor::TransactionByCreatedAtCursor;
use repo::*;

#[derive(Clone)]
pub struct Transactions {
    repo: TransactionRepo,
}

impl Transactions {
    pub(crate) fn new(pool: &PgPool) -> Self {
        Self {
            repo: TransactionRepo::new(pool),
        }
    }

    #[instrument(
        level = "debug",
        name = "cala_ledger.transactions.find_by_external_id",
        skip(self)
    )]
    pub async fn find_by_external_id(
        &self,
        external_id: String,
    ) -> Result<Transaction, TransactionError> {
        Ok(self.repo.find_by_external_id(Some(external_id)).await?)
    }

    #[instrument(
        level = "debug",
        name = "cala_ledger.transactions.find_by_id",
        skip(self)
    )]
    pub async fn find_by_id(
        &self,
        transaction_id: TransactionId,
    ) -> Result<Transaction, TransactionError> {
        Ok(self.repo.find_by_id(transaction_id).await?)
    }

    #[instrument(
        level = "debug",
        name = "cala_ledger.transactions.list_for_template_id",
        skip(self)
    )]
    pub async fn list_for_template_id(
        &self,
        template_id: TxTemplateId,
        query: es_entity::PaginatedQueryArgs<TransactionByCreatedAtCursor>,
        direction: es_entity::ListDirection,
    ) -> Result<
        es_entity::PaginatedQueryRet<Transaction, TransactionByCreatedAtCursor>,
        TransactionError,
    > {
        Ok(self
            .repo
            .list_for_tx_template_id_by_created_at(template_id, query, direction)
            .await?)
    }

    #[instrument(level = "debug", name = "cala_ledger.transactions.find_all", skip(self, transaction_ids), fields(transaction_ids_count = transaction_ids.len()))]
    pub async fn find_all<T: From<Transaction>>(
        &self,
        transaction_ids: &[TransactionId],
    ) -> Result<HashMap<TransactionId, T>, TransactionError> {
        Ok(self.repo.find_all(transaction_ids).await?)
    }
}

impl From<&TransactionEvent> for OutboxEventPayload {
    fn from(event: &TransactionEvent) -> Self {
        match event {
            TransactionEvent::Initialized {
                values: transaction,
            } => OutboxEventPayload::TransactionCreated {
                transaction: transaction.clone(),
            },
            TransactionEvent::Updated { values, fields } => {
                OutboxEventPayload::TransactionUpdated {
                    transaction: values.clone(),
                    fields: fields.clone(),
                }
            }
        }
    }
}
