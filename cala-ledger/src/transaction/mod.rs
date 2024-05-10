pub mod error;

mod entity;
mod repo;

use sqlx::PgPool;
use tracing::instrument;

#[cfg(feature = "import")]
use crate::primitives::DataSourceId;
use crate::{
    entity::*,
    outbox::*,
    primitives::{DataSource, TxTemplateId},
};

pub use entity::*;
use error::*;
use repo::*;

#[derive(Clone)]
pub struct Transactions {
    repo: TransactionRepo,
    outbox: Outbox,
    pool: PgPool,
}

impl Transactions {
    pub fn new(pool: &PgPool, outbox: Outbox) -> Self {
        Self {
            repo: TransactionRepo::new(pool),
            outbox,
            pool: pool.clone(),
        }
    }
    #[instrument(name = "cala_ledger.transactions.create", skip(self))]
    pub async fn create(
        &self,
        new_transaction: NewTransaction,
    ) -> Result<Transaction, TransactionError> {
        let mut tx = self.pool.begin().await?;
        let EntityUpdate {
            entity: transaction,
            n_new_events,
        } = self.repo.create_in_tx(&mut tx, new_transaction).await?;
        self.outbox
            .persist_events(tx, transaction.events.last_persisted(n_new_events))
            .await?;
        Ok(transaction)
    }

    #[instrument(name = "cala_ledger.transactions.list_by_template_id", skip(self))]
    pub async fn list_by_template_id(
        &self,
        tx_template_id: TxTemplateId,
    ) -> Result<Vec<Transaction>, TransactionError> {
        self.repo.list_by_template_id(tx_template_id).await
    }

    #[cfg(feature = "import")]
    pub async fn sync_transaction_creation(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        origin: DataSourceId,
        values: TransactionValues,
    ) -> Result<(), TransactionError> {
        let transaction = Transaction::import(origin, values);
        self.repo.import(tx, origin, transaction).await
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
