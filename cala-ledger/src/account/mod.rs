//! [Account] holds a balance in a [Journal](crate::journal::Journal)
mod entity;
pub mod error;
mod repo;

use sqlx::PgPool;
use tracing::instrument;

use std::collections::HashMap;

#[cfg(feature = "import")]
use crate::primitives::DataSourceId;
use crate::{
    outbox::*,
    primitives::{DataSource, Status},
};

pub use entity::*;
use error::*;
pub use repo::account_cursor::*;
use repo::*;

/// Service for working with `Account` entities.
#[derive(Clone)]
pub struct Accounts {
    repo: AccountRepo,
}

impl Accounts {
    pub(crate) fn new(pool: &PgPool, publisher: &OutboxPublisher) -> Self {
        Self {
            repo: AccountRepo::new(pool, publisher),
        }
    }

    #[instrument(name = "cala_ledger.accounts.create", skip_all)]
    pub async fn create(&self, new_account: NewAccount) -> Result<Account, AccountError> {
        let mut op = self.repo.begin_op().await?;
        let account = self.create_in_op(&mut op, new_account).await?;
        op.commit().await?;
        Ok(account)
    }

    #[instrument(name = "cala_ledger.accounts.create_in_op", skip(self, db))]
    pub async fn create_in_op(
        &self,
        db: &mut impl es_entity::AtomicOperation,
        new_account: NewAccount,
    ) -> Result<Account, AccountError> {
        let account = self.repo.create_in_op(db, new_account).await?;
        Ok(account)
    }

    #[instrument(name = "cala_ledger.accounts.create_all", skip_all)]
    pub async fn create_all(
        &self,
        new_accounts: Vec<NewAccount>,
    ) -> Result<Vec<Account>, AccountError> {
        let mut op = self.repo.begin_op().await?;
        let accounts = self.create_all_in_op(&mut op, new_accounts).await?;
        op.commit().await?;
        Ok(accounts)
    }

    #[instrument(name = "cala_ledger.accounts.create_all_in_op", skip(self, db))]
    pub async fn create_all_in_op(
        &self,
        db: &mut impl es_entity::AtomicOperation,
        new_accounts: Vec<NewAccount>,
    ) -> Result<Vec<Account>, AccountError> {
        let accounts = self.repo.create_all_in_op(db, new_accounts).await?;
        Ok(accounts)
    }

    #[instrument(name = "cala_ledger.accounts.find", skip_all)]
    pub async fn find(&self, account_id: AccountId) -> Result<Account, AccountError> {
        self.repo.find_by_id(account_id).await
    }

    #[instrument(name = "cala_ledger.accounts.find_all", skip(self))]
    pub async fn find_all<T: From<Account>>(
        &self,
        account_ids: &[AccountId],
    ) -> Result<HashMap<AccountId, T>, AccountError> {
        self.repo.find_all(account_ids).await
    }

    #[instrument(name = "cala_ledger.accounts.find_all", skip(self, db))]
    pub async fn find_all_in_op<T: From<Account>>(
        &self,
        db: &mut impl es_entity::AtomicOperation,
        account_ids: &[AccountId],
    ) -> Result<HashMap<AccountId, T>, AccountError> {
        self.repo.find_all_in_op(db, account_ids).await
    }

    #[instrument(name = "cala_ledger.accounts.find_by_external_id", skip(self))]
    pub async fn find_by_external_id(&self, external_id: String) -> Result<Account, AccountError> {
        self.repo.find_by_external_id(Some(external_id)).await
    }

    #[instrument(name = "cala_ledger.accounts.find_by_code", skip(self))]
    pub async fn find_by_code(&self, code: String) -> Result<Account, AccountError> {
        self.repo.find_by_code(code).await
    }

    #[instrument(name = "cala_ledger.accounts.list", skip(self))]
    pub async fn list(
        &self,
        query: es_entity::PaginatedQueryArgs<AccountsByNameCursor>,
    ) -> Result<es_entity::PaginatedQueryRet<Account, AccountsByNameCursor>, AccountError> {
        self.repo.list_by_name(query, Default::default()).await
    }

    #[instrument(name = "cala_ledger.accounts.lock_in_op", skip(self, db))]
    pub async fn lock_in_op(
        &self,
        db: &mut impl es_entity::AtomicOperation,
        id: AccountId,
    ) -> Result<(), AccountError> {
        let mut account = self.repo.find_by_id_in_op(&mut *db, id).await?;
        if account.update_status(Status::Locked).did_execute() {
            self.persist_in_op(db, &mut account).await?;
        }
        Ok(())
    }

    #[instrument(name = "cala_ledger.accounts.unlock_in_op", skip(self, db))]
    pub async fn unlock_in_op(
        &self,
        db: &mut impl es_entity::AtomicOperation,
        id: AccountId,
    ) -> Result<(), AccountError> {
        let mut account = self.repo.find_by_id_in_op(&mut *db, id).await?;
        if account.update_status(Status::Active).did_execute() {
            self.persist_in_op(db, &mut account).await?;
        }
        Ok(())
    }

    #[instrument(name = "cala_ledger.accounts.persist", skip(self, account))]
    pub async fn persist(&self, account: &mut Account) -> Result<(), AccountError> {
        let mut op = self.repo.begin_op().await?;
        self.persist_in_op(&mut op, account).await?;
        op.commit().await?;
        Ok(())
    }

    #[instrument(name = "cala_ledger.accounts.persist_in_op", skip_all)]
    pub async fn persist_in_op(
        &self,
        db: &mut impl es_entity::AtomicOperation,
        account: &mut Account,
    ) -> Result<(), AccountError> {
        if account.is_account_set() {
            return Err(AccountError::CannotUpdateAccountSetAccounts);
        }
        self.repo.update_in_op(db, account).await?;
        Ok(())
    }

    #[instrument(
        name = "cala_ledger.accounts.update_velocity_context_values_in_op",
        skip_all
    )]
    pub(crate) async fn update_velocity_context_values_in_op(
        &self,
        db: &mut impl es_entity::AtomicOperation,
        values: impl Into<VelocityContextAccountValues>,
    ) -> Result<(), AccountError> {
        self.repo
            .update_velocity_context_values_in_op(db, values.into())
            .await
    }

    #[cfg(feature = "import")]
    #[instrument(name = "cala_ledger.accounts.sync_account_creation", skip_all)]
    pub async fn sync_account_creation(
        &self,
        mut db: es_entity::DbOpWithTime<'_>,
        origin: DataSourceId,
        values: AccountValues,
    ) -> Result<(), AccountError> {
        let mut account = Account::import(origin, values);
        self.repo
            .import_in_op(&mut db, origin, &mut account)
            .await?;
        db.commit().await?;
        Ok(())
    }

    #[cfg(feature = "import")]
    #[instrument(name = "cala_ledger.accounts.sync_account_update", skip_all)]
    pub async fn sync_account_update(
        &self,
        mut db: es_entity::DbOpWithTime<'_>,
        values: AccountValues,
        fields: Vec<String>,
    ) -> Result<(), AccountError> {
        let mut account = self.repo.find_by_id_in_op(&mut db, values.id).await?;
        let _ = account.update((values, fields));
        self.repo.update_in_op(&mut db, &mut account).await?;
        db.commit().await?;
        Ok(())
    }
}

impl From<&AccountEvent> for OutboxEventPayload {
    fn from(event: &AccountEvent) -> Self {
        let source = es_entity::context::EventContext::current()
            .data()
            .lookup("data_source")
            .ok()
            .flatten()
            .unwrap_or(DataSource::Local);

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
                source,
                account: account.clone(),
            },
            AccountEvent::Updated {
                values: account,
                fields,
            } => OutboxEventPayload::AccountUpdated {
                source,
                account: account.clone(),
                fields: fields.clone(),
            },
        }
    }
}
