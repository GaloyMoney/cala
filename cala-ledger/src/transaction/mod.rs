pub mod error;

mod entity;
mod repo;

use sqlx::{PgPool, Postgres, Transaction as DbTransaction};

#[cfg(feature = "import")]
use crate::primitives::DataSourceId;
use crate::{entity::EntityUpdate, outbox::*, primitives::DataSource};

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

    pub(crate) async fn create_in_tx(
        &self,
        tx: &mut DbTransaction<'_, Postgres>,
        new_transaction: NewTransaction,
    ) -> Result<(Transaction, OutboxEventPayload), TransactionError> {
        let EntityUpdate {
            entity: transaction,
            ..
        } = self.repo.create_in_tx(tx, new_transaction).await?;
        let event = transaction
            .events
            .last_persisted(1)
            .next()
            .expect("should have event")
            .into();
        Ok((transaction, event))
    }

    #[cfg(feature = "import")]
    pub async fn sync_transaction_creation(
        &self,
        mut tx: sqlx::Transaction<'_, sqlx::Postgres>,
        origin: DataSourceId,
        values: TransactionValues,
    ) -> Result<(), TransactionError> {
        let mut transaction = Transaction::import(origin, values);
        self.repo.import(&mut tx, origin, &mut transaction).await?;
        self.outbox
            .persist_events(tx, transaction.events.last_persisted(1))
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
