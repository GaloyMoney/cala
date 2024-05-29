mod entity;
pub mod error;
mod repo;

#[cfg(feature = "import")]
use chrono::{DateTime, Utc};
use sqlx::PgPool;
use std::collections::HashMap;
use tracing::instrument;

#[cfg(feature = "import")]
use crate::primitives::DataSourceId;
use crate::{
    account::*,
    atomic_operation::*,
    outbox::*,
    primitives::{DataSource, JournalId},
};

pub use entity::*;
use error::*;
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

    pub async fn add_member_in_op(
        &self,
        op: &mut AtomicOperation<'_>,
        account_set_id: AccountSetId,
        member: impl Into<AccountSetMember>,
    ) -> Result<AccountSet, AccountSetError> {
        let member = member.into();
        let account_set = match member {
            AccountSetMember::Account(id) => {
                self.repo
                    .add_member_account(op.tx(), account_set_id, id)
                    .await?;
                self.repo.find(account_set_id).await?
            }
            AccountSetMember::AccountSet(id) => {
                let mut accounts = self
                    .repo
                    .find_all::<AccountSet>(&[account_set_id, id])
                    .await?;
                let target = accounts
                    .remove(&account_set_id)
                    .ok_or(AccountSetError::CouldNotFindById(account_set_id))?;
                let member = accounts
                    .remove(&id)
                    .ok_or(AccountSetError::CouldNotFindById(id))?;

                if target.values().journal_id != member.values().journal_id {
                    return Err(AccountSetError::JournalIdMissmatch);
                }

                self.repo
                    .add_member_set(op.tx(), account_set_id, id)
                    .await?;
                target
            }
        };

        op.accumulate(std::iter::once(
            OutboxEventPayload::AccountSetMemberCreated {
                source: DataSource::Local,
                account_set_id,
                member,
            },
        ));

        //
        // check balances
        // create entries
        // update balances (including mappings)
        //

        Ok(account_set)
    }

    #[instrument(name = "cala_ledger.account_sets.find_all", skip(self), err)]
    pub async fn find_all<T: From<AccountSet>>(
        &self,
        account_set_ids: &[AccountSetId],
    ) -> Result<HashMap<AccountSetId, T>, AccountSetError> {
        self.repo.find_all(account_set_ids).await
    }

    pub(crate) async fn fetch_mappings(
        &self,
        journal_id: JournalId,
        account_ids: &[AccountId],
    ) -> Result<HashMap<AccountId, Vec<AccountSetId>>, AccountSetError> {
        self.repo.fetch_mappings(journal_id, account_ids).await
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

    #[cfg(feature = "import")]
    pub async fn sync_account_set_member_creation(
        &self,
        mut db: sqlx::Transaction<'_, sqlx::Postgres>,
        recorded_at: DateTime<Utc>,
        origin: DataSourceId,
        account_set_id: AccountSetId,
        member: AccountSetMember,
    ) -> Result<(), AccountSetError> {
        match member {
            AccountSetMember::Account(account_id) => {
                self.repo
                    .import_member_account(&mut db, recorded_at, origin, account_set_id, account_id)
                    .await?;
            }
            AccountSetMember::AccountSet(account_set_id) => {
                self.repo
                    .import_member_set(&mut db, recorded_at, origin, account_set_id, account_set_id)
                    .await?;
            }
        }
        self.outbox
            .persist_events_at(
                db,
                std::iter::once(OutboxEventPayload::AccountSetMemberCreated {
                    source: DataSource::Remote { id: origin },
                    account_set_id,
                    member,
                }),
                recorded_at,
            )
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
