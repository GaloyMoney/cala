mod entity;
pub mod error;
mod member;
mod repo;

#[cfg(feature = "import")]
use chrono::{DateTime, Utc};
use sqlx::{PgPool, Postgres, Transaction as DbTransaction};
use tracing::instrument;

#[cfg(feature = "import")]
use crate::primitives::DataSourceId;
use crate::{account::*, atomic_operation::*, outbox::*, primitives::DataSource};

pub use entity::*;
use error::*;
pub use member::*;
use repo::*;

#[derive(Clone)]
pub struct AccountSets {
    repo: AccountSetRepo,
    accounts: Accounts,
    outbox: Outbox,
    pool: PgPool,
}

impl AccountSets {
    pub(crate) fn new(pool: &PgPool, outbox: Outbox, accounts: &Accounts) -> Self {
        Self {
            repo: AccountSetRepo::new(pool),
            outbox,
            accounts: accounts.clone(),
            pool: pool.clone(),
        }
    }
    #[instrument(name = "cala_ledger.account_sets.create", skip(self))]
    pub async fn create(
        &self,
        new_account_set: NewAccountSet,
    ) -> Result<AccountSet, AccountSetError> {
        let mut op = AtomicOperation::init(&self.pool, &self.outbox).await?;
        let account_set = self.create_in_op(&mut op, new_account_set).await?;
        op.commit().await?;
        Ok(account_set)
    }

    #[instrument(name = "cala_ledger.account_sets.create", skip(self, op))]
    pub async fn create_in_op(
        &self,
        op: &mut AtomicOperation<'_>,
        new_account_set: NewAccountSet,
    ) -> Result<AccountSet, AccountSetError> {
        let new_account = NewAccount::builder()
            .id(uuid::Uuid::from(new_account_set.id))
            .name(String::new())
            .code(new_account_set.id.to_string())
            .normal_balance_type(new_account_set.normal_balance_type)
            .is_account_set(true)
            .build()
            .expect("Failed to build account");
        self.accounts.create_in_op(op, new_account).await?;
        let account_set = self.repo.create_in_tx(op.tx(), new_account_set).await?;
        op.accumulate(account_set.events.last_persisted());
        Ok(account_set)
    }

    pub async fn add_to_account_set_in_tx(
        &self,
        db: &mut DbTransaction<'_, Postgres>,
        account_set_id: AccountSetId,
        member: AccountSetMember,
    ) -> Result<AccountSet, AccountSetError> {
        let account_set = self.repo.find(account_set_id).await?;
        let AccountSetMember::Account(account_id) = member;
        self.repo
            .add_member_account(db, account_set_id, account_id)
            .await?;
        Ok(account_set)
    }

    #[cfg(feature = "import")]
    pub async fn sync_account_set_creation(
        &self,
        mut db: sqlx::Transaction<'_, sqlx::Postgres>,
        recorded_at: DateTime<Utc>,
        origin: DataSourceId,
        values: AccountSetValues,
    ) -> Result<(), AccountSetError> {
        let mut account_set = AccountSet::import(origin, values);
        self.repo
            .import(&mut db, recorded_at, origin, &mut account_set)
            .await?;
        self.outbox
            .persist_events_at(db, account_set.events.last_persisted(), recorded_at)
            .await?;
        Ok(())
    }
}

impl From<&AccountSetEvent> for OutboxEventPayload {
    fn from(event: &AccountSetEvent) -> Self {
        match event {
            #[cfg(feature = "import")]
            AccountSetEvent::Imported {
                source,
                values: account_set,
            } => OutboxEventPayload::AccountSetCreated {
                source: *source,
                account_set: account_set.clone(),
            },
            AccountSetEvent::Initialized {
                values: account_set,
            } => OutboxEventPayload::AccountSetCreated {
                source: DataSource::Local,
                account_set: account_set.clone(),
            },
        }
    }
}
