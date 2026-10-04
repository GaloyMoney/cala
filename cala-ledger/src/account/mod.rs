//! [Account] holds a balance in a [Journal](crate::journal::Journal)
use es_entity::errlanes::{lanes, Fail, ResultExt};
mod entity;
pub mod error;
mod repo;

use es_entity::clock::ClockHandle;
use sqlx::PgPool;

use std::collections::HashMap;

use crate::{
    account_set_member::AccountSetMembers,
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

    #[es_entity::errlanes::instrument(
        level = "debug",
        name = "cala_ledger.accounts.create",
        skip_all
    )]
    pub async fn create(
        &self,
        new_account: NewAccount,
    ) -> Result<Account, Fail<CreateAccountRejection, lanes!(Transient, Fatal)>> {
        let mut op = self.repo.begin_op_with_clock(&self.clock).await?;
        let account = self.create_in_op(&mut op, new_account).await?;
        op.commit().await?;
        Ok(account)
    }

    #[es_entity::errlanes::instrument(
        level = "debug",
        name = "cala_ledger.accounts.create_in_op",
        skip(self, db),
        fields(initial_set_count = new_account.initial_account_set.is_some() as u8)
    )]
    pub async fn create_in_op(
        &self,
        db: &mut impl es_entity::AtomicOperation,
        new_account: NewAccount,
    ) -> Result<Account, Fail<CreateAccountRejection, lanes!(Transient, Fatal)>> {
        let pairs = initial_membership_pairs(std::slice::from_ref(&new_account));
        let account = self.insert_in_op(db, new_account).await.widen()?;
        self.attach_initial_account_set_in_op(db, pairs)
            .await
            .widen()?;
        Ok(account)
    }

    #[es_entity::errlanes::instrument(
        level = "debug",
        name = "cala_ledger.accounts.create_all",
        skip_all
    )]
    pub async fn create_all(
        &self,
        new_accounts: Vec<NewAccount>,
    ) -> Result<Vec<Account>, Fail<CreateAccountRejection, lanes!(Transient, Fatal)>> {
        let mut op = self.repo.begin_op_with_clock(&self.clock).await?;
        let accounts = self.create_all_in_op(&mut op, new_accounts).await?;
        op.commit().await?;
        Ok(accounts)
    }

    #[es_entity::errlanes::instrument(level = "debug", name = "cala_ledger.accounts.create_all_in_op", skip(self, db, new_accounts), fields(count = new_accounts.len(), initial_set_count = tracing::field::Empty))]
    pub async fn create_all_in_op(
        &self,
        db: &mut impl es_entity::AtomicOperation,
        new_accounts: Vec<NewAccount>,
    ) -> Result<Vec<Account>, Fail<CreateAccountRejection, lanes!(Transient, Fatal)>> {
        let pairs = initial_membership_pairs(&new_accounts);
        tracing::Span::current().record("initial_set_count", pairs.len());
        let accounts = self.insert_all_in_op(db, new_accounts).await.widen()?;
        self.attach_initial_account_set_in_op(db, pairs)
            .await
            .widen()?;
        Ok(accounts)
    }

    #[es_entity::errlanes::instrument(
        level = "debug",
        name = "cala_ledger.accounts.find",
        skip_all
    )]
    pub async fn find(
        &self,
        account_id: AccountId,
    ) -> Result<Account, Fail<AccountNotFound, lanes!(Transient, Fatal)>> {
        Ok(self
            .repo
            .maybe_find_by_id(account_id)
            .await?
            .ok_or(AccountNotFound(account_id))?)
    }

    #[es_entity::errlanes::instrument(level = "debug", name = "cala_ledger.accounts.find_all", skip(self, account_ids), fields(account_ids_count = account_ids.len()))]
    pub async fn find_all<T: From<Account>>(
        &self,
        account_ids: &[AccountId],
    ) -> Result<HashMap<AccountId, T>, crate::CalaFault> {
        self.repo.find_all(account_ids).await
    }

    #[es_entity::errlanes::instrument(level = "debug", name = "cala_ledger.accounts.find_all_in_op", skip(self, db, account_ids), fields(account_ids_count = account_ids.len()))]
    pub async fn find_all_in_op<T: From<Account>>(
        &self,
        db: impl es_entity::IntoOneTimeExecutor<'_>,
        account_ids: &[AccountId],
    ) -> Result<HashMap<AccountId, T>, crate::CalaFault> {
        self.repo.find_all_in_op(db, account_ids).await
    }

    #[es_entity::errlanes::instrument(
        level = "debug",
        name = "cala_ledger.accounts.find_by_external_id",
        skip(self)
    )]
    pub async fn find_by_external_id(
        &self,
        external_id: String,
    ) -> Result<Account, Fail<AccountExternalIdNotFound, lanes!(Transient, Fatal)>> {
        Ok(self
            .repo
            .maybe_find_by_external_id(Some(external_id.clone()))
            .await?
            .ok_or(AccountExternalIdNotFound(external_id))?)
    }

    #[es_entity::errlanes::instrument(
        level = "debug",
        name = "cala_ledger.accounts.find_by_code",
        skip(self)
    )]
    pub async fn find_by_code(
        &self,
        code: String,
    ) -> Result<Account, Fail<AccountCodeNotFound, lanes!(Transient, Fatal)>> {
        Ok(self
            .repo
            .maybe_find_by_code(code.clone())
            .await?
            .ok_or(AccountCodeNotFound(code))?)
    }

    #[es_entity::errlanes::instrument(
        level = "debug",
        name = "cala_ledger.accounts.lock_in_op",
        skip(self, db)
    )]
    pub async fn lock_in_op(
        &self,
        db: &mut impl es_entity::AtomicOperation,
        id: AccountId,
    ) -> Result<(), Fail<SetAccountStatusRejection, lanes!(Transient, Fatal)>> {
        let mut account = self
            .repo
            .maybe_find_by_id_in_op(&mut *db, id)
            .await?
            .ok_or(AccountNotFound(id))?;
        if account.update_status(Status::Locked).did_execute() {
            self.persist_in_op(db, &mut account).await.widen()?;
        }
        Ok(())
    }

    #[es_entity::errlanes::instrument(
        level = "debug",
        name = "cala_ledger.accounts.unlock_in_op",
        skip(self, db)
    )]
    pub async fn unlock_in_op(
        &self,
        db: &mut impl es_entity::AtomicOperation,
        id: AccountId,
    ) -> Result<(), Fail<SetAccountStatusRejection, lanes!(Transient, Fatal)>> {
        let mut account = self
            .repo
            .maybe_find_by_id_in_op(&mut *db, id)
            .await?
            .ok_or(AccountNotFound(id))?;
        if account.update_status(Status::Active).did_execute() {
            self.persist_in_op(db, &mut account).await.widen()?;
        }
        Ok(())
    }

    #[es_entity::errlanes::instrument(
        level = "debug",
        name = "cala_ledger.accounts.persist",
        skip(self, account)
    )]
    pub async fn persist(
        &self,
        account: &mut Account,
    ) -> Result<(), Fail<PersistAccountRejection, lanes!(Transient, Fatal)>> {
        let mut op = self.repo.begin_op_with_clock(&self.clock).await?;
        self.persist_in_op(&mut op, account).await?;
        op.commit().await?;
        Ok(())
    }

    #[es_entity::errlanes::instrument(
        level = "debug",
        name = "cala_ledger.accounts.persist_in_op",
        skip_all
    )]
    pub async fn persist_in_op(
        &self,
        db: &mut impl es_entity::AtomicOperation,
        account: &mut Account,
    ) -> Result<(), Fail<PersistAccountRejection, lanes!(Transient, Fatal)>> {
        if account.is_account_set() {
            return Err(PersistAccountRejection::CannotUpdateAccountSetAccounts.into());
        }
        self.repo.update_in_op(db, account).await.widen()?;
        Ok(())
    }

    async fn insert_in_op(
        &self,
        db: &mut impl es_entity::AtomicOperation,
        account: NewAccount,
    ) -> Result<Account, Fail<InsertAccountRejection, lanes!(Transient, Fatal)>> {
        self.repo.create_in_op(db, account).await.widen()
    }

    async fn insert_all_in_op(
        &self,
        db: &mut impl es_entity::AtomicOperation,
        accounts: Vec<NewAccount>,
    ) -> Result<Vec<Account>, Fail<InsertAccountRejection, lanes!(Transient, Fatal)>> {
        self.repo.create_all_in_op(db, accounts).await.widen()
    }

    pub(crate) async fn create_backing_in_op(
        &self,
        db: &mut impl es_entity::AtomicOperation,
        account: BackingAccount,
    ) -> Result<(), Fail<CreateBackingAccountRejection, lanes!(Transient, Fatal)>> {
        self.repo
            .create_in_op(db, account.into_new())
            .await
            .widen()?;
        Ok(())
    }

    pub(crate) async fn create_all_backing_in_op(
        &self,
        db: &mut impl es_entity::AtomicOperation,
        accounts: Vec<BackingAccount>,
    ) -> Result<(), Fail<CreateBackingAccountRejection, lanes!(Transient, Fatal)>> {
        self.repo
            .create_all_in_op(
                db,
                accounts.into_iter().map(BackingAccount::into_new).collect(),
            )
            .await
            .widen()?;
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
    ) -> Result<(), Fail<InitialAccountSetNotFound, lanes!(Transient, Fatal)>> {
        if pairs.is_empty() {
            return Ok(());
        }

        self.account_set_members
            .attach_new_accounts_in_op(db, &pairs)
            .await
            .map_rejected(InitialAccountSetNotFound::from_missing)
    }

    #[es_entity::errlanes::instrument(
        level = "debug",
        name = "cala_ledger.accounts.update_velocity_context_values_in_op",
        skip_all
    )]
    pub(crate) async fn update_velocity_context_values_in_op(
        &self,
        db: &mut impl es_entity::AtomicOperation,
        values: impl Into<VelocityContextAccountValues>,
    ) -> Result<(), crate::CalaFault> {
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

/// The only inputs accepted by the account-set backing insert. No external ID
/// or initial membership can enter this path.
pub(crate) struct BackingAccount {
    pub id: AccountSetId,
    pub normal_balance_type: crate::primitives::DebitOrCredit,
    pub eventually_consistent: bool,
    pub context: VelocityContextAccountValues,
}

impl BackingAccount {
    fn into_new(self) -> NewAccount {
        NewAccount::builder()
            .id(self.id)
            .name(String::new())
            .code(self.id.to_string())
            .normal_balance_type(self.normal_balance_type)
            .is_account_set(true)
            .eventually_consistent(self.eventually_consistent)
            .velocity_context_values(self.context)
            .build()
            .expect("backing account fields are complete")
    }
}
