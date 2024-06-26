//! [Account] holds a balance in a [Journal](crate::journal::Journal)
mod cursor;
mod entity;
pub mod error;
mod repo;

#[cfg(feature = "import")]
use chrono::{DateTime, Utc};
use sqlx::PgPool;
use tracing::instrument;

use std::collections::HashMap;

#[cfg(feature = "import")]
use crate::primitives::DataSourceId;
use crate::{atomic_operation::*, outbox::*, primitives::DataSource, query::*};

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
    pub(crate) fn new(pool: &PgPool, outbox: Outbox) -> Self {
        Self {
            repo: AccountRepo::new(pool),
            outbox,
            pool: pool.clone(),
        }
    }

    #[instrument(name = "cala_ledger.accounts.create", skip(self))]
    pub async fn create(&self, new_account: NewAccount) -> Result<Account, AccountError> {
        let mut op = AtomicOperation::init(&self.pool, &self.outbox).await?;
        let account = self.create_in_op(&mut op, new_account).await?;
        op.commit().await?;
        Ok(account)
    }

    pub async fn create_in_op(
        &self,
        op: &mut AtomicOperation<'_>,
        new_account: NewAccount,
    ) -> Result<Account, AccountError> {
        let account = self.repo.create_in_tx(op.tx(), new_account).await?;
        op.accumulate(account.events.last_persisted());
        Ok(account)
    }

    pub async fn find(&self, account_id: AccountId) -> Result<Account, AccountError> {
        self.repo.find(account_id).await
    }

    #[instrument(name = "cala_ledger.accounts.find_all", skip(self), err)]
    pub async fn find_all<T: From<Account>>(
        &self,
        account_ids: &[AccountId],
    ) -> Result<HashMap<AccountId, T>, AccountError> {
        self.repo.find_all(account_ids).await
    }

    #[instrument(name = "cala_ledger.accounts.find_all", skip(self, op), err)]
    pub async fn find_all_in_op<T: From<Account>>(
        &self,
        op: &mut AtomicOperation<'_>,
        account_ids: &[AccountId],
    ) -> Result<HashMap<AccountId, T>, AccountError> {
        self.repo.find_all_in_tx(op.tx(), account_ids).await
    }

    #[instrument(name = "cala_ledger.accounts.find_by_external_id", skip(self), err)]
    pub async fn find_by_external_id(&self, external_id: String) -> Result<Account, AccountError> {
        self.repo.find_by_external_id(external_id).await
    }

    #[instrument(name = "cala_ledger.accounts.find_by_code", skip(self), err)]
    pub async fn find_by_code(&self, code: String) -> Result<Account, AccountError> {
        self.repo.find_by_code(code).await
    }

    #[instrument(name = "cala_ledger.accounts.list", skip(self))]
    pub async fn list(
        &self,
        query: PaginatedQueryArgs<AccountByNameCursor>,
    ) -> Result<PaginatedQueryRet<Account, AccountByNameCursor>, AccountError> {
        self.repo.list(query).await
    }

    #[instrument(name = "cala_ledger.accounts.persist", skip(self, account))]
    pub async fn persist(&self, account: &mut Account) -> Result<(), AccountError> {
        let mut op = AtomicOperation::init(&self.pool, &self.outbox).await?;
        self.persist_in_op(&mut op, account).await?;
        op.commit().await?;
        Ok(())
    }

    pub async fn persist_in_op(
        &self,
        op: &mut AtomicOperation<'_>,
        account: &mut Account,
    ) -> Result<(), AccountError> {
        self.repo.persist_in_tx(op.tx(), account).await?;
        op.accumulate(account.events.last_persisted());
        Ok(())
    }

    #[cfg(feature = "import")]
    pub async fn sync_account_creation(
        &self,
        mut db: sqlx::Transaction<'_, sqlx::Postgres>,
        recorded_at: DateTime<Utc>,
        origin: DataSourceId,
        values: AccountValues,
    ) -> Result<(), AccountError> {
        let mut account = Account::import(origin, values);
        self.repo
            .import(&mut db, recorded_at, origin, &mut account)
            .await?;
        self.outbox
            .persist_events_at(db, account.events.last_persisted(), recorded_at)
            .await?;
        Ok(())
    }

    #[cfg(feature = "import")]
    pub async fn sync_account_update(
        &self,
        mut db: sqlx::Transaction<'_, sqlx::Postgres>,
        recorded_at: DateTime<Utc>,
        origin: DataSourceId,
        values: AccountValues,
        fields: Vec<String>,
    ) -> Result<(), AccountError> {
        let mut account = self.repo.find_imported(values.id, origin).await?;
        account.update((values, fields));
        self.repo
            .persist_at_in_tx(&mut db, recorded_at, origin, &mut account)
            .await?;
        self.outbox
            .persist_events_at(db, account.events.last_persisted(), recorded_at)
            .await?;
        Ok(())
    }
}

impl From<&AccountEvent> for OutboxEventPayload {
    fn from(event: &AccountEvent) -> Self {
        match event {
            #[cfg(feature = "import")]
            AccountEvent::Imported {
                source,
                values: account,
            } => OutboxEventPayload::AccountCreated {
                source: *source,
                account: account.clone(),
            },
            AccountEvent::Initialized { values: account } => OutboxEventPayload::AccountCreated {
                source: DataSource::Local,
                account: account.clone(),
            },
            AccountEvent::Updated {
                values: account,
                fields,
            } => OutboxEventPayload::AccountUpdated {
                source: DataSource::Local,
                account: account.clone(),
                fields: fields.clone(),
            },
        }
    }
}
