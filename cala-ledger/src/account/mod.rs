//! [Account] holds a balance in a [Journal](crate::journal::Journal)
mod cursor;
mod entity;
mod repo;

#[cfg(feature = "import")]
use chrono::{DateTime, Utc};
use sqlx::PgPool;
use tracing::instrument;

use std::collections::HashMap;

#[cfg(feature = "import")]
use crate::primitives::DataSourceId;
use crate::{entity::*, errors::*, outbox::*, primitives::DataSource, query::*};

pub use cursor::*;
pub use entity::*;
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
    pub async fn create(
        &self,
        new_account: NewAccount,
    ) -> Result<Account, OneOf<(ConstraintVioliation, UnexpectedDbError)>> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| OneOf::new(UnexpectedDbError(e)))?;
        let EntityUpdate {
            entity: account,
            n_new_events,
        } = self.repo.create_in_tx(&mut tx, new_account).await?;
        self.outbox
            .persist_events(tx, account.events.last_persisted(n_new_events))
            .await
            .map_err(OneOf::broaden)?;
        Ok(account)
    }

    pub async fn find(
        &self,
        account_id: AccountId,
    ) -> Result<Account, OneOf<(EntityNotFound, HydratingEntityError, UnexpectedDbError)>> {
        self.repo.find(account_id).await
    }

    #[instrument(name = "cala_ledger.accounts.find_all", skip(self), err)]
    pub async fn find_all(
        &self,
        account_ids: &[AccountId],
    ) -> Result<HashMap<AccountId, AccountValues>, OneOf<(HydratingEntityError, UnexpectedDbError)>>
    {
        self.repo.find_all(account_ids).await
    }

    pub async fn find_by_external_id(
        &self,
        external_id: String,
    ) -> Result<Account, OneOf<(EntityNotFound, HydratingEntityError, UnexpectedDbError)>> {
        self.repo.find_by_external_id(external_id).await
    }

    #[instrument(name = "cala_ledger.accounts.list", skip(self))]
    pub async fn list(
        &self,
        query: PaginatedQueryArgs<AccountByNameCursor>,
    ) -> Result<
        PaginatedQueryRet<Account, AccountByNameCursor>,
        OneOf<(HydratingEntityError, UnexpectedDbError)>,
    > {
        self.repo.list(query).await
    }

    #[cfg(feature = "import")]
    pub async fn sync_account_creation(
        &self,
        mut tx: sqlx::Transaction<'_, sqlx::Postgres>,
        recorded_at: DateTime<Utc>,
        origin: DataSourceId,
        values: AccountValues,
    ) -> Result<(), OneOf<(UnexpectedDbError,)>> {
        let mut account = Account::import(origin, values);
        self.repo
            .import(&mut tx, recorded_at, origin, &mut account)
            .await?;
        self.outbox
            .persist_events_at(tx, account.events.last_persisted(1), recorded_at)
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
        }
    }
}
