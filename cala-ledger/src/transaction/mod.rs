pub mod error;

mod entity;
mod repo;

use sqlx::PgPool;
use tracing::instrument;

use std::collections::HashMap;

use crate::primitives::{EntryId, TxTemplateId};
use crate::outbox::*;

pub use entity::*;
use error::*;
pub use repo::transaction_cursor::TransactionsByCreatedAtCursor;
use repo::*;

#[derive(Clone)]
pub struct Transactions {
    repo: TransactionRepo,
}

impl Transactions {
    pub(crate) fn new(pool: &PgPool, publisher: &OutboxPublisher) -> Self {
        Self {
            repo: TransactionRepo::new(pool, publisher),
        }
    }

    #[instrument(name = "cala_ledger.transactions.create_in_op", skip_all)]
    pub(crate) async fn create_in_op(
        &self,
        db: &mut impl es_entity::AtomicOperation,
        new_transaction: NewTransaction,
    ) -> Result<Transaction, TransactionError> {
        let transaction = self.repo.create_in_op(db, new_transaction).await?;
        Ok(transaction)
    }

    #[instrument(name = "cala_ledger.transactions.create_voided_tx_in_op", skip_all)]
    pub async fn create_voided_tx_in_op(
        &self,
        db: &mut impl es_entity::AtomicOperationWithTime,
        voiding_tx_id: TransactionId,
        existing_tx_id: TransactionId,
        entry_ids: impl IntoIterator<Item = EntryId>,
    ) -> Result<Transaction, TransactionError> {
        let mut existing_tx = self.repo.find_by_id_in_op(&mut *db, existing_tx_id).await?;

        let new_tx = existing_tx.void(voiding_tx_id, entry_ids.into_iter().collect(), db.now())?;

        self.repo.update_in_op(db, &mut existing_tx).await?;
        let voided_tx = self.repo.create_in_op(db, new_tx).await?;

        Ok(voided_tx)
    }

    #[instrument(name = "cala_ledger.transactions.find_by_external_id", skip(self))]
    pub async fn find_by_external_id(
        &self,
        external_id: String,
    ) -> Result<Transaction, TransactionError> {
        self.repo.find_by_external_id(Some(external_id)).await
    }

    #[instrument(name = "cala_ledger.transactions.find_by_id", skip(self))]
    pub async fn find_by_id(
        &self,
        transaction_id: TransactionId,
    ) -> Result<Transaction, TransactionError> {
        self.repo.find_by_id(transaction_id).await
    }

    #[instrument(name = "cala_ledger.transactions.list_for_template_id", skip(self))]
    pub async fn list_for_template_id(
        &self,
        template_id: TxTemplateId,
        query: es_entity::PaginatedQueryArgs<TransactionsByCreatedAtCursor>,
        direction: es_entity::ListDirection,
    ) -> Result<
        es_entity::PaginatedQueryRet<Transaction, TransactionsByCreatedAtCursor>,
        TransactionError,
    > {
        self.repo
            .list_for_tx_template_id_by_created_at(template_id, query, direction)
            .await
    }

    #[instrument(name = "cala_ledger.transactions.find_all", skip(self))]
    pub async fn find_all<T: From<Transaction>>(
        &self,
        transaction_ids: &[TransactionId],
    ) -> Result<HashMap<TransactionId, T>, TransactionError> {
        self.repo.find_all(transaction_ids).await
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
