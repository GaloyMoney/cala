//! [Account] holds a balance in a [Journal](crate::journal::Journal)
mod entity;
pub mod error;
mod repo;

use es_entity::clock::ClockHandle;
use sqlx::PgPool;
use tracing::instrument;

use std::collections::HashMap;

use crate::{
    account_set_member::{AccountSetMemberError, AccountSetMembers},
    outbox::*,
    primitives::{AccountSetId, Status},
};

pub use entity::*;
use error::*;
pub use repo::account_cursor::*;
use repo::*;

/// Service for working with `Account` entities.
#[derive(Clone)]
pub struct Accounts {
    repo: AccountRepo,
    account_set_members: AccountSetMembers,
    clock: ClockHandle,
}

impl Accounts {
    pub(crate) fn new(
        pool: &PgPool,
        publisher: &OutboxPublisher,
        account_set_members: &AccountSetMembers,
        clock: &ClockHandle,
    ) -> Self {
        Self {
            repo: AccountRepo::new(pool, publisher),
            account_set_members: account_set_members.clone(),
            clock: clock.clone(),
        }
    }

    #[instrument(level = "debug", name = "cala_ledger.accounts.create", skip_all)]
    pub async fn create(&self, new_account: NewAccount) -> Result<Account, AccountError> {
        let mut op = self.repo.begin_op_with_clock(&self.clock).await?;
        let account = self.create_in_op(&mut op, new_account).await?;
        op.commit().await?;
        Ok(account)
    }

    #[instrument(
        level = "debug",
        name = "cala_ledger.accounts.create_in_op",
        skip(self, db),
        fields(initial_set_count = new_account.initial_account_set.is_some() as u8)
    )]
    pub async fn create_in_op(
        &self,
        db: &mut impl es_entity::AtomicOperation,
        new_account: NewAccount,
    ) -> Result<Account, AccountError> {
        let pairs = initial_membership_pairs(std::slice::from_ref(&new_account));
        let account = self.repo.create_in_op(db, new_account).await?;
        self.attach_initial_account_set_in_op(db, pairs).await?;
        Ok(account)
    }

    #[instrument(level = "debug", name = "cala_ledger.accounts.create_all", skip_all)]
    pub async fn create_all(
        &self,
        new_accounts: Vec<NewAccount>,
    ) -> Result<Vec<Account>, AccountError> {
        let mut op = self.repo.begin_op_with_clock(&self.clock).await?;
        let accounts = self.create_all_in_op(&mut op, new_accounts).await?;
        op.commit().await?;
        Ok(accounts)
    }

    #[instrument(level = "debug", name = "cala_ledger.accounts.create_all_in_op", skip(self, db, new_accounts), fields(count = new_accounts.len(), initial_set_count = tracing::field::Empty))]
    pub async fn create_all_in_op(
        &self,
        db: &mut impl es_entity::AtomicOperation,
        new_accounts: Vec<NewAccount>,
    ) -> Result<Vec<Account>, AccountError> {
        let pairs = initial_membership_pairs(&new_accounts);
        tracing::Span::current().record("initial_set_count", pairs.len());
        let accounts = self.repo.create_all_in_op(db, new_accounts).await?;
        self.attach_initial_account_set_in_op(db, pairs).await?;
        Ok(accounts)
    }

    #[instrument(level = "debug", name = "cala_ledger.accounts.find", skip_all)]
    pub async fn find(&self, account_id: AccountId) -> Result<Account, AccountError> {
        Ok(self.repo.find_by_id(account_id).await?)
    }

    #[instrument(level = "debug", name = "cala_ledger.accounts.find_all", skip(self, account_ids), fields(account_ids_count = account_ids.len()))]
    pub async fn find_all<T: From<Account>>(
        &self,
        account_ids: &[AccountId],
    ) -> Result<HashMap<AccountId, T>, AccountError> {
        Ok(self.repo.find_all(account_ids).await?)
    }

    #[instrument(level = "debug", name = "cala_ledger.accounts.find_all_in_op", skip(self, db, account_ids), fields(account_ids_count = account_ids.len()))]
    pub async fn find_all_in_op<T: From<Account>>(
        &self,
        db: &mut impl es_entity::AtomicOperation,
        account_ids: &[AccountId],
    ) -> Result<HashMap<AccountId, T>, AccountError> {
        Ok(self.repo.find_all_in_op(db, account_ids).await?)
    }

    #[instrument(
        level = "debug",
        name = "cala_ledger.accounts.find_by_external_id",
        skip(self)
    )]
    pub async fn find_by_external_id(&self, external_id: String) -> Result<Account, AccountError> {
        Ok(self.repo.find_by_external_id(Some(external_id)).await?)
    }

    #[instrument(
        level = "debug",
        name = "cala_ledger.accounts.find_by_code",
        skip(self)
    )]
    pub async fn find_by_code(&self, code: String) -> Result<Account, AccountError> {
        Ok(self.repo.find_by_code(code).await?)
    }

    #[instrument(
        level = "debug",
        name = "cala_ledger.accounts.lock_in_op",
        skip(self, db)
    )]
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

    #[instrument(
        level = "debug",
        name = "cala_ledger.accounts.unlock_in_op",
        skip(self, db)
    )]
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

    #[instrument(
        level = "debug",
        name = "cala_ledger.accounts.persist",
        skip(self, account)
    )]
    pub async fn persist(&self, account: &mut Account) -> Result<(), AccountError> {
        let mut op = self.repo.begin_op_with_clock(&self.clock).await?;
        self.persist_in_op(&mut op, account).await?;
        op.commit().await?;
        Ok(())
    }

    #[instrument(level = "debug", name = "cala_ledger.accounts.persist_in_op", skip_all)]
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

    /// The create-inside-set fast path: write the direct membership for
    /// an account created *in this same op* via
    /// [`NewAccount::initial_account_set`], through
    /// `crate::account_set_member::AccountSetMembers::attach_new_accounts_in_op`
    /// — one statement (lock + insert; the account-set FK is the
    /// existence check).
    ///
    /// This takes NEITHER the coarse membership-graph lock nor the
    /// class-1 balance-history guard lock — only the class-2 per-member
    /// EXCLUSIVE — and runs no balance-history or path-uniqueness check.
    /// The invariant argument for why that is sound (and its accepted
    /// caveat) lives on the `NewAccount::initial_account_set` field
    /// docs; the restriction that makes it hold is enforced at the type
    /// level: [`initial_membership_pairs`] can produce at most one pair
    /// per account.
    ///
    /// Statement footprint when every field is empty: ZERO — the create
    /// path is byte-identical to a plain create. When set: exactly one
    /// added statement.
    async fn attach_initial_account_set_in_op(
        &self,
        db: &mut impl es_entity::AtomicOperation,
        pairs: Vec<(AccountSetId, AccountId)>,
    ) -> Result<(), AccountError> {
        if pairs.is_empty() {
            return Ok(());
        }

        self.account_set_members
            .attach_new_accounts_in_op(db, &pairs)
            .await
            .map_err(|e| match e {
                AccountSetMemberError::AccountSetsNotFound(missing) => {
                    AccountError::InitialAccountSetNotFound(
                        *missing.first().expect("missing ids are never empty"),
                    )
                }
                AccountSetMemberError::Sqlx(e) => AccountError::Sqlx(e),
            })
    }

    #[instrument(
        level = "debug",
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
}

/// Partition the fast-path membership pairs out of a batch of
/// `NewAccount`s. `NewAccount::initial_account_set` is an `Option`, so at
/// most one pair per account falls out here by construction — see its
/// field docs for why k≥2 initial sets cannot be made lock-free at all.
/// Accounts with no set are simply created without membership.
fn initial_membership_pairs(new_accounts: &[NewAccount]) -> Vec<(AccountSetId, AccountId)> {
    new_accounts
        .iter()
        .filter_map(|new_account| {
            new_account
                .initial_account_set
                .map(|set_id| (set_id, new_account.id))
        })
        .collect()
}

impl From<&AccountEvent> for OutboxEventPayload {
    fn from(event: &AccountEvent) -> Self {
        match event {
            AccountEvent::Initialized { values: account } => OutboxEventPayload::AccountCreated {
                account: account.clone(),
            },
            AccountEvent::Updated {
                values: account,
                fields,
            } => OutboxEventPayload::AccountUpdated {
                account: account.clone(),
                fields: fields.clone(),
            },
        }
    }
}
