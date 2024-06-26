pub mod error;

mod entity;
mod repo;

#[cfg(feature = "import")]
use chrono::{DateTime, Utc};
use sqlx::PgPool;
use tracing::instrument;

use std::collections::HashMap;

#[cfg(feature = "import")]
use crate::primitives::DataSourceId;
use crate::{atomic_operation::*, outbox::*, primitives::DataSource};

pub use entity::*;
use error::*;
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
        op: &mut AtomicOperation<'_>,
        new_transaction: NewTransaction,
    ) -> Result<Transaction, TransactionError> {
        let transaction = self.repo.create_in_tx(op.tx(), new_transaction).await?;
        op.accumulate(transaction.events.last_persisted());
        Ok(transaction)
    }

    #[instrument(name = "cala_ledger.transactions.find_by_external_id", skip(self), err)]
    pub async fn find_by_external_id(
        &self,
        external_id: String,
    ) -> Result<Transaction, TransactionError> {
        self.repo.find_by_external_id(external_id).await
    }

    #[instrument(name = "cala_ledger.transactions.find_by_id", skip(self), err)]
    pub async fn find_by_id(
        &self,
        transaction_id: TransactionId,
    ) -> Result<Transaction, TransactionError> {
        self.repo.find_by_id(transaction_id).await
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
        mut db: sqlx::Transaction<'_, sqlx::Postgres>,
        recorded_at: DateTime<Utc>,
        origin: DataSourceId,
        values: TransactionValues,
    ) -> Result<(), TransactionError> {
        let mut transaction = Transaction::import(origin, values);
        self.repo
            .import(&mut db, recorded_at, origin, &mut transaction)
            .await?;
        self.outbox
            .persist_events_at(db, transaction.events.last_persisted(), recorded_at)
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
