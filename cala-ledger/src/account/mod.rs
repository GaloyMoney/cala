//! [Account] holds a balance in a [Journal](crate::journal::Journal)
mod entity;
pub mod error;
mod repo;

use es_entity::EsEntity;
use sqlx::PgPool;
use tracing::instrument;

use std::collections::HashMap;

#[cfg(feature = "import")]
use crate::primitives::DataSourceId;
use crate::{
    ledger_operation::*,
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

    #[instrument(name = "cala_ledger.accounts.create", skip_all)]
    pub async fn create(&self, new_account: NewAccount) -> Result<Account, AccountError> {
        let mut op = LedgerOperation::init(&self.pool, &self.outbox).await?;
        let account = self.create_in_op(&mut op, new_account).await?;
        op.commit().await?;
        Ok(account)
    }

    #[instrument(name = "cala_ledger.accounts.create_in_op", skip(self, db))]
    pub async fn create_in_op(
        &self,
        db: &mut LedgerOperation<'_>,
        new_account: NewAccount,
    ) -> Result<Account, AccountError> {
        let account = self.repo.create_in_op(db, new_account).await?;
        db.accumulate(account.last_persisted(1).map(|p| &p.event));
        Ok(account)
    }

    #[instrument(name = "cala_ledger.accounts.create_all", skip_all)]
    pub async fn create_all(
        &self,
        new_accounts: Vec<NewAccount>,
    ) -> Result<Vec<Account>, AccountError> {
        let mut op = LedgerOperation::init(&self.pool, &self.outbox).await?;
        let accounts = self.create_all_in_op(&mut op, new_accounts).await?;
        op.commit().await?;
        Ok(accounts)
    }

    #[instrument(name = "cala_ledger.accounts.create_all_in_op", skip(self, db))]
    pub async fn create_all_in_op(
        &self,
        db: &mut LedgerOperation<'_>,
        new_accounts: Vec<NewAccount>,
    ) -> Result<Vec<Account>, AccountError> {
        let accounts = self.repo.create_all_in_op(db, new_accounts).await?;
        db.accumulate(
            accounts
                .iter()
                .flat_map(|account| account.last_persisted(1).map(|p| &p.event)),
        );
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
        db: &mut LedgerOperation<'_>,
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
        db: &mut LedgerOperation<'_>,
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
        db: &mut LedgerOperation<'_>,
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
        let mut op = LedgerOperation::init(&self.pool, &self.outbox).await?;
        self.persist_in_op(&mut op, account).await?;
        op.commit().await?;
        Ok(())
    }

    #[instrument(name = "cala_ledger.accounts.persist_in_op", skip_all)]
    pub async fn persist_in_op(
        &self,
        db: &mut LedgerOperation<'_>,
        account: &mut Account,
    ) -> Result<(), AccountError> {
        if account.is_account_set() {
            return Err(AccountError::CannotUpdateAccountSetAccounts);
        }

        let n_events = self.repo.update_in_op(db, account).await?;
        db.accumulate(account.last_persisted(n_events).map(|p| &p.event));
        Ok(())
    }

    #[instrument(
        name = "cala_ledger.accounts.update_velocity_context_values_in_op",
        skip_all
    )]
    pub(crate) async fn update_velocity_context_values_in_op(
        &self,
        db: &mut LedgerOperation<'_>,
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
        let outbox_events: Vec<_> = account
            .last_persisted(1)
            .map(|p| OutboxEventPayload::from(&p.event))
            .collect();
        let now = db.now();
        self.outbox
            .persist_events_at(db, outbox_events, now)
            .await?;
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
        let mut account = self.repo.find_by_id(values.id).await?;
        let _ = account.update((values, fields));
        let n_events = self.repo.update_in_op(&mut db, &mut account).await?;
        let outbox_events: Vec<_> = account
            .last_persisted(n_events)
            .map(|p| OutboxEventPayload::from(&p.event))
            .collect();
        let time = db.now();
        self.outbox
            .persist_events_at(db, outbox_events, time)
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
