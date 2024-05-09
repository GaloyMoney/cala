//! [Account] holds a balance in a [Journal](crate::journal::Journal)
mod cursor;
mod entity;
pub mod error;
mod repo;

use sqlx::PgPool;
use tracing::instrument;

use crate::{
    entity::*,
    outbox::*,
    primitives::{DataSource, DataSourceId},
    query::*,
};

pub use cursor::*;
pub use entity::*;
use error::*;
use repo::*;

/// Service for working with `Account` entities.
#[derive(Clone)]
pub struct Accounts {
    repo: AccountRepo,
    outbox: Outbox,
    pool: PgPool,
}

impl Accounts {
    pub fn new(pool: &PgPool, outbox: Outbox) -> Self {
        Self {
            repo: AccountRepo::new(pool),
            outbox,
            pool: pool.clone(),
        }
    }

    #[instrument(name = "cala_ledger.accounts.create", skip(self))]
    pub async fn create(&self, new_account: NewAccount) -> Result<Account, AccountError> {
        let mut tx = self.pool.begin().await?;
        let EntityUpdate {
            entity: account,
            n_new_events,
        } = self.repo.create_in_tx(&mut tx, new_account).await?;
        self.outbox
            .persist_events(tx, account.events.last_persisted(n_new_events))
            .await?;
        Ok(account)
    }

    #[instrument(name = "cala_ledger.accounts.list", skip(self))]
    pub async fn list(
        &self,
        query: PaginatedQueryArgs<AccountByNameCursor>,
    ) -> Result<PaginatedQueryRet<Account, AccountByNameCursor>, AccountError> {
        self.repo.list(query).await
    }

    pub async fn sync_account_creation(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        origin: DataSourceId,
        values: AccountValues,
    ) -> Result<(), AccountError> {
        Ok(())
    }
}

impl From<&AccountEvent> for OutboxEventPayload {
    fn from(event: &AccountEvent) -> Self {
        match event {
            AccountEvent::Initialized { values: account } => OutboxEventPayload::AccountCreated {
                source: DataSource::Local,
                account: account.clone(),
            },
        }
    }
}
