pub mod error;

mod entity;
mod repo;

use es_entity::EsEntity;
use sqlx::PgPool;
use tracing::instrument;

use std::collections::HashMap;

#[cfg(feature = "import")]
use crate::primitives::DataSourceId;
use crate::primitives::{EntryId, TxTemplateId};
use crate::{ledger_operation::*, outbox::*, primitives::DataSource};

pub use entity::*;
use error::*;
pub use repo::transaction_cursor::TransactionsByCreatedAtCursor;
use repo::*;

#[derive(Clone)]
pub struct Transactions {
    repo: TransactionRepo,
    outbox: Outbox,
    _pool: PgPool,
}

impl Transactions {
    pub(crate) fn new(pool: &PgPool, outbox: Outbox) -> Self {
        Self {
            repo: TransactionRepo::new(pool),
            outbox,
            _pool: pool.clone(),
        }
    }

    pub(crate) async fn create_in_op(
        &self,
        db: &mut LedgerOperation<'_>,
        new_transaction: NewTransaction,
    ) -> Result<Transaction, TransactionError> {
        let transaction = self.repo.create_in_op(db.op(), new_transaction).await?;
        db.accumulate(transaction.last_persisted(1).map(|p| &p.event));
        Ok(transaction)
    }

    pub async fn create_voided_tx_in_op(
        &self,
        db: &mut LedgerOperation<'_>,
        original_tx_id: TransactionId,
        new_tx_id: TransactionId,
        entry_ids: impl IntoIterator<Item = EntryId>,
    ) -> Result<Transaction, TransactionError> {
        let original_tx_values = self.repo.find_by_id(original_tx_id).await?.into_values();

        let mut builder = NewTransaction::builder();
        builder
            .id(new_tx_id)
            .created_at(db.op().now())
            .tx_template_id(original_tx_values.tx_template_id)
            .entry_ids(entry_ids.into_iter().collect())
            .effective(chrono::Utc::now().date_naive())
            .journal_id(original_tx_values.journal_id)
            .correlation_id(original_tx_values.correlation_id);

        if let Some(external_id) = original_tx_values.external_id {
            builder.external_id(external_id);
        }
        if let Some(description) = original_tx_values.description {
            builder.description(description);
        }
        if let Some(metadata) = original_tx_values.metadata {
            builder.metadata(metadata);
        }
        let new_transaction = builder.build().expect("Couldn't build voided transaction");

        let transaction = self.create_in_op(db, new_transaction).await?;
        db.accumulate(transaction.last_persisted(1).map(|p| &p.event));
        Ok(transaction)
    }

    #[instrument(name = "cala_ledger.transactions.find_by_external_id", skip(self), err)]
    pub async fn find_by_external_id(
        &self,
        external_id: String,
    ) -> Result<Transaction, TransactionError> {
        self.repo.find_by_external_id(Some(external_id)).await
    }

    #[instrument(name = "cala_ledger.transactions.find_by_id", skip(self), err)]
    pub async fn find_by_id(
        &self,
        transaction_id: TransactionId,
    ) -> Result<Transaction, TransactionError> {
        self.repo.find_by_id(transaction_id).await
    }

    #[instrument(
        name = "cala_ledger.transactions.list_for_template_id",
        skip(self),
        err
    )]
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

    #[instrument(name = "cala_ledger.transactions.find_all", skip(self), err)]
    pub async fn find_all<T: From<Transaction>>(
        &self,
        transaction_ids: &[TransactionId],
    ) -> Result<HashMap<TransactionId, T>, TransactionError> {
        self.repo.find_all(transaction_ids).await
    }

    #[cfg(feature = "import")]
    pub async fn sync_transaction_creation(
        &self,
        mut db: es_entity::DbOp<'_>,
        origin: DataSourceId,
        values: TransactionValues,
    ) -> Result<(), TransactionError> {
        let mut transaction = Transaction::import(origin, values);
        self.repo
            .import_in_op(&mut db, origin, &mut transaction)
            .await?;
        let recorded_at = db.now();
        let outbox_events: Vec<_> = transaction
            .last_persisted(1)
            .map(|p| OutboxEventPayload::from(&p.event))
            .collect();
        self.outbox
            .persist_events_at(db.into_tx(), outbox_events, recorded_at)
            .await?;
        Ok(())
    }
}

impl From<&TransactionEvent> for OutboxEventPayload {
    fn from(event: &TransactionEvent) -> Self {
        match event {
            #[cfg(feature = "import")]
            TransactionEvent::Imported {
                source,
                values: transaction,
            } => OutboxEventPayload::TransactionCreated {
                source: *source,
                transaction: transaction.clone(),
            },
            TransactionEvent::Initialized {
                values: transaction,
            } => OutboxEventPayload::TransactionCreated {
                source: DataSource::Local,
                transaction: transaction.clone(),
            },
        }
    }
}
