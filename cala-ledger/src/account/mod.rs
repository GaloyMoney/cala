//! [Account] holds a balance in a [Journal](crate::journal::Journal)
mod entity;
pub mod error;
mod repo;

use sqlx::PgPool;
use tracing::instrument;

use std::collections::HashMap;

#[cfg(feature = "import")]
use crate::primitives::DataSourceId;
use crate::{ledger_operation::*, outbox::*, primitives::DataSource};

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

    #[instrument(name = "cala_ledger.accounts.create", skip(self))]
    pub async fn create(&self, new_account: NewAccount) -> Result<Account, AccountError> {
        let mut op = LedgerOperation::init(&self.pool, &self.outbox).await?;
        let account = self.create_in_op(&mut op, new_account).await?;
        op.commit().await?;
        Ok(account)
    }

    pub async fn create_in_op(
        &self,
        db: &mut LedgerOperation<'_>,
        new_account: NewAccount,
    ) -> Result<Account, AccountError> {
        let account = self.repo.create_in_op(db.op(), new_account).await?;
        db.accumulate(account.events.last_persisted(1).map(|p| &p.event));
        Ok(account)
    }

    pub async fn find(&self, account_id: AccountId) -> Result<Account, AccountError> {
        self.repo.find_by_id(account_id).await
    }

    #[instrument(name = "cala_ledger.accounts.find_all", skip(self), err)]
    pub async fn find_all<T: From<Account>>(
        &self,
        account_ids: &[AccountId],
    ) -> Result<HashMap<AccountId, T>, AccountError> {
        self.repo.find_all(account_ids).await
    }

    #[instrument(name = "cala_ledger.accounts.find_all", skip(self, db), err)]
    pub async fn find_all_in_op<T: From<Account>>(
        &self,
        db: &mut LedgerOperation<'_>,
        account_ids: &[AccountId],
    ) -> Result<HashMap<AccountId, T>, AccountError> {
        self.repo.find_all_in_tx(db.tx(), account_ids).await
    }

    #[instrument(name = "cala_ledger.accounts.find_by_external_id", skip(self), err)]
    pub async fn find_by_external_id(&self, external_id: String) -> Result<Account, AccountError> {
        self.repo.find_by_external_id(Some(external_id)).await
    }

    #[instrument(name = "cala_ledger.accounts.find_by_code", skip(self), err)]
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

    #[instrument(name = "cala_ledger.accounts.persist", skip(self, account))]
    pub async fn persist(&self, account: &mut Account) -> Result<(), AccountError> {
        let mut op = LedgerOperation::init(&self.pool, &self.outbox).await?;
        self.persist_in_op(&mut op, account).await?;
        op.commit().await?;
        Ok(())
    }

    pub async fn persist_in_op(
        &self,
        db: &mut LedgerOperation<'_>,
        account: &mut Account,
    ) -> Result<(), AccountError> {
        self.repo.update_in_op(db.op(), account).await?;
        db.accumulate(account.events.last_persisted(1).map(|p| &p.event));
        Ok(())
    }

    #[cfg(feature = "import")]
    pub async fn sync_account_creation(
        &self,
        mut db: es_entity::DbOp<'_>,
        origin: DataSourceId,
        values: AccountValues,
    ) -> Result<(), AccountError> {
        let mut account = Account::import(origin, values);
        self.repo
            .import_in_op(&mut db, origin, &mut account)
            .await?;
        // let recorded_at = db.now();
        // self.outbox
        //     .persist_events_at(
        //         db.into_tx(),
        //         account.events.last_persisted(1).map(|p| &p.event),
        //         recorded_at,
        //     )
        //     .await?;
        Ok(())
    }

    #[cfg(feature = "import")]
    pub async fn sync_account_update(
        &self,
        mut db: es_entity::DbOp<'_>,
        values: AccountValues,
        fields: Vec<String>,
    ) -> Result<(), AccountError> {
        let mut account = self.repo.find_by_id(values.id).await?;
        account.update((values, fields));
        self.repo.update_in_op(&mut db, &mut account).await?;
        let recorded_at = db.now();
        self.outbox
            .persist_events_at(
                db.into_tx(),
                account.events.last_persisted(1).map(|p| &p.event),
                recorded_at,
            )
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
