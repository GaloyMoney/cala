mod entity;
pub mod error;
mod repo;

use es_entity::clock::ClockHandle;
use sqlx::PgPool;
use std::collections::HashMap;
use tracing::instrument;

use crate::{account::*, balance::*, outbox::*, primitives::JournalId};

pub use entity::*;
use error::*;
use repo::*;
pub use repo::{account_set_cursor::*, members_cursor::*};

#[derive(Clone)]
pub struct AccountSets {
    repo: AccountSetRepo,
    accounts: Accounts,
    balances: Balances,
    clock: ClockHandle,
}

impl AccountSets {
    pub(crate) fn new(
        pool: &PgPool,
        publisher: &OutboxPublisher,
        accounts: &Accounts,
        balances: &Balances,
        clock: &ClockHandle,
    ) -> Self {
        Self {
            repo: AccountSetRepo::new(pool, publisher),
            accounts: accounts.clone(),
            balances: balances.clone(),
            clock: clock.clone(),
        }
    }
    #[instrument(level = "debug", name = "cala_ledger.account_sets.create", skip(self))]
    pub async fn create(
        &self,
        new_account_set: NewAccountSet,
    ) -> Result<AccountSet, AccountSetError> {
        let mut op = self.repo.begin_op_with_clock(&self.clock).await?;
        let account_set = self.create_in_op(&mut op, new_account_set).await?;
        op.commit().await?;
        Ok(account_set)
    }

    #[instrument(
        level = "debug",
        name = "cala_ledger.account_sets.create_in_op",
        skip(self, db)
    )]
    pub async fn create_in_op(
        &self,
        db: &mut impl es_entity::AtomicOperation,
        new_account_set: NewAccountSet,
    ) -> Result<AccountSet, AccountSetError> {
        let new_account = NewAccount::builder()
            .id(new_account_set.id)
            .name(String::new())
            .code(new_account_set.id.to_string())
            .normal_balance_type(new_account_set.normal_balance_type)
            .is_account_set(true)
            .eventually_consistent(new_account_set.is_eventually_consistent())
            .velocity_context_values(new_account_set.context_values())
            .build()
            .expect("Failed to build account");
        self.accounts.create_in_op(db, new_account).await?;

        let account_set = self.repo.create_in_op(db, new_account_set).await?;

        Ok(account_set)
    }

    #[instrument(level = "debug", name = "cala_ledger.account_sets.create_all", skip(self, new_account_sets), fields(count = new_account_sets.len()))]
    pub async fn create_all(
        &self,
        new_account_sets: Vec<NewAccountSet>,
    ) -> Result<Vec<AccountSet>, AccountSetError> {
        let mut op = self.repo.begin_op_with_clock(&self.clock).await?;
        let account_sets = self.create_all_in_op(&mut op, new_account_sets).await?;
        op.commit().await?;
        Ok(account_sets)
    }

    #[instrument(level = "debug", name = "cala_ledger.account_sets.create_all_in_op", skip(self, db, new_account_sets), fields(count = new_account_sets.len()))]
    pub async fn create_all_in_op(
        &self,
        db: &mut impl es_entity::AtomicOperation,
        new_account_sets: Vec<NewAccountSet>,
    ) -> Result<Vec<AccountSet>, AccountSetError> {
        let mut new_accounts = Vec::new();
        for new_account_set in new_account_sets.iter() {
            let new_account = NewAccount::builder()
                .id(new_account_set.id)
                .name(String::new())
                .code(new_account_set.id.to_string())
                .normal_balance_type(new_account_set.normal_balance_type)
                .is_account_set(true)
                .eventually_consistent(new_account_set.is_eventually_consistent())
                .velocity_context_values(new_account_set.context_values())
                .build()
                .expect("Failed to build account");
            new_accounts.push(new_account);
        }
        self.accounts.create_all_in_op(db, new_accounts).await?;

        let account_sets = self.repo.create_all_in_op(db, new_account_sets).await?;

        Ok(account_sets)
    }

    #[instrument(
        level = "debug",
        name = "cala_ledger.account_sets.persist",
        skip(self, account_set)
    )]
    pub async fn persist(&self, account_set: &mut AccountSet) -> Result<(), AccountSetError> {
        let mut op = self.repo.begin_op_with_clock(&self.clock).await?;
        self.persist_in_op(&mut op, account_set).await?;
        op.commit().await?;
        Ok(())
    }

    #[instrument(
        level = "debug",
        name = "cala_ledger.account_sets.persist_in_op",
        skip(self, db, account_set)
    )]
    pub async fn persist_in_op(
        &self,
        db: &mut impl es_entity::AtomicOperation,
        account_set: &mut AccountSet,
    ) -> Result<(), AccountSetError> {
        self.repo.update_in_op(db, account_set).await?;

        self.accounts
            .update_velocity_context_values_in_op(db, account_set.values())
            .await?;

        Ok(())
    }

    #[instrument(level = "debug", name = "cala_ledger.account_sets.add_member", skip(self, member), fields(account_set_id = %account_set_id))]
    pub async fn add_member(
        &self,
        account_set_id: AccountSetId,
        member: impl Into<AccountSetMemberId>,
    ) -> Result<AccountSet, AccountSetError> {
        let mut op = self.repo.begin_op_with_clock(&self.clock).await?;
        let account_set = self
            .add_member_in_op(&mut op, account_set_id, member)
            .await?;
        op.commit().await?;
        Ok(account_set)
    }

    #[instrument(
        level = "debug",
        name = "cala_ledger.account_sets.add_member_in_op",
        skip(self, op, member),
        fields(
            account_set_id = %account_set_id,
            is_account = tracing::field::Empty,
            is_account_set = tracing::field::Empty,
            member_id = tracing::field::Empty,
        ),
        err(level = "warn")
    )]
    pub async fn add_member_in_op(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        account_set_id: AccountSetId,
        member: impl Into<AccountSetMemberId>,
    ) -> Result<AccountSet, AccountSetError> {
        let member = member.into();

        // Resolve the target set (and, for set-member, verify the journal
        // matches) without writing the membership row, so we can run the
        // no-history check first.
        let (account_set, member_id) = match member {
            AccountSetMemberId::Account(id) => {
                tracing::Span::current().record("is_account", true);
                tracing::Span::current().record("is_account_set", false);
                tracing::Span::current().record("member_id", tracing::field::display(&id));
                let set = self.repo.find_by_id_in_op(&mut *op, account_set_id).await?;
                (set, id)
            }
            AccountSetMemberId::AccountSet(id) => {
                tracing::Span::current().record("is_account", false);
                tracing::Span::current().record("is_account_set", true);
                tracing::Span::current().record("member_id", tracing::field::display(&id));
                let mut sets = self
                    .repo
                    .find_all_in_op::<AccountSet>(&mut *op, &[account_set_id, id])
                    .await?;
                let target = sets
                    .remove(&account_set_id)
                    .ok_or(AccountSetError::CouldNotFindById(account_set_id))?;
                let member_set = sets
                    .remove(&id)
                    .ok_or(AccountSetError::CouldNotFindById(id))?;

                if target.values().journal_id != member_set.values().journal_id {
                    return Err(AccountSetError::JournalIdMismatch);
                }

                (target, AccountId::from(id))
            }
        };

        self.assert_member_history_empty_in_op(
            op,
            account_set_id,
            account_set.values().journal_id,
            AccountId::from(&account_set.id()),
            member_id,
        )
        .await?;

        match member {
            AccountSetMemberId::Account(id) => {
                self.repo
                    .assert_members_absent_in_op(op, &[(account_set_id, id)])
                    .await?;
                self.repo
                    .add_member_account(&mut *op, account_set_id, id)
                    .await?;
            }
            AccountSetMemberId::AccountSet(id) => {
                self.repo
                    .assert_member_set_absent_in_op(op, account_set_id, id)
                    .await?;
                self.repo.add_member_set(op, account_set_id, id).await?;
            }
        }

        Ok(account_set)
    }

    #[instrument(level = "debug", name = "cala_ledger.account_sets.add_members", skip(self, members), fields(count = members.len()))]
    pub async fn add_members(
        &self,
        members: &[(AccountSetId, AccountId)],
    ) -> Result<(), AccountSetError> {
        let mut op = self.repo.begin_op_with_clock(&self.clock).await?;
        self.add_members_in_op(&mut op, members).await?;
        op.commit().await?;
        Ok(())
    }

    /// Batch variant of [`add_member_in_op`](Self::add_member_in_op) for
    /// account members: resolves all target sets, runs the
    /// no-balance-history check for every pair, and inserts all direct
    /// memberships in one statement. Ancestor rows are materialized
    /// asynchronously by the fill job. Callers attaching many accounts at
    /// once should prefer this over looping `add_member_in_op`.
    #[instrument(
        level = "debug",
        name = "cala_ledger.account_sets.add_members_in_op",
        skip(self, op, members),
        fields(count = members.len()),
        err(level = "warn")
    )]
    pub async fn add_members_in_op(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        members: &[(AccountSetId, AccountId)],
    ) -> Result<(), AccountSetError> {
        if members.is_empty() {
            return Ok(());
        }

        let account_set_ids: Vec<AccountSetId> =
            members.iter().map(|(set_id, _)| *set_id).collect();
        let sets = self
            .repo
            .find_all_in_op::<AccountSet>(&mut *op, &account_set_ids)
            .await?;

        let mut check_pairs = Vec::with_capacity(members.len());
        for (account_set_id, member_id) in members {
            let set = sets
                .get(account_set_id)
                .ok_or(AccountSetError::CouldNotFindById(*account_set_id))?;
            check_pairs.push((
                set.values().journal_id,
                AccountId::from(set.id()),
                *member_id,
            ));
        }
        let with_history = self
            .balances
            .members_with_balance_history_in_op(op, &check_pairs)
            .await?;
        if let Some(member_id) = with_history.into_iter().next() {
            let (account_set_id, _) = members
                .iter()
                .find(|(_, m)| *m == member_id)
                .expect("member with history must be in input");
            return Err(AccountSetError::MemberHasBalanceHistory {
                account_set_id: *account_set_id,
                member_id,
            });
        }

        self.repo.assert_members_absent_in_op(op, members).await?;

        self.repo.add_member_accounts(op, members).await?;

        Ok(())
    }

    /// `cala_balance_history` row in `journal_id`. Folding existing
    /// balance into a parent set after the fact is unsafe under
    /// concurrent posters and EC recalcs (the watermark advance can leap
    /// past unprocessed history of *other* members), and the symmetric
    /// remove case has no safe unfold path either, so we forbid both.
    ///
    /// The check itself is run under exclusive locks on the parent set
    /// and the candidate member in the EC-set lock namespace, so the
    /// existence query reflects committed state even with concurrent
    /// posters in flight.
    async fn assert_member_history_empty_in_op(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        account_set_id: AccountSetId,
        journal_id: JournalId,
        target_account_id: AccountId,
        member_id: AccountId,
    ) -> Result<(), AccountSetError> {
        if self
            .balances
            .member_has_balance_history_in_op(op, journal_id, target_account_id, member_id)
            .await?
        {
            return Err(AccountSetError::MemberHasBalanceHistory {
                account_set_id,
                member_id,
            });
        }
        Ok(())
    }

    #[instrument(level = "debug", name = "cala_ledger.account_sets.remove_member", skip(self, member), fields(account_set_id = %account_set_id))]
    pub async fn remove_member(
        &self,
        account_set_id: AccountSetId,
        member: impl Into<AccountSetMemberId>,
    ) -> Result<AccountSet, AccountSetError> {
        let mut op = self.repo.begin_op_with_clock(&self.clock).await?;
        let account_set = self
            .remove_member_in_op(&mut op, account_set_id, member)
            .await?;
        op.commit().await?;
        Ok(account_set)
    }

    #[instrument(
        level = "debug",
        name = "cala_ledger.account_sets.remove_member_in_op",
        skip(self, op, member),
        fields(account_set_id = %account_set_id),
        err(level = "warn")
    )]
    pub async fn remove_member_in_op(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        account_set_id: AccountSetId,
        member: impl Into<AccountSetMemberId>,
    ) -> Result<AccountSet, AccountSetError> {
        let member = member.into();

        let (account_set, member_id) = match member {
            AccountSetMemberId::Account(id) => {
                let set = self.repo.find_by_id_in_op(&mut *op, account_set_id).await?;
                (set, id)
            }
            AccountSetMemberId::AccountSet(id) => {
                let mut sets = self
                    .repo
                    .find_all_in_op::<AccountSet>(&mut *op, &[account_set_id, id])
                    .await?;
                let target = sets
                    .remove(&account_set_id)
                    .ok_or(AccountSetError::CouldNotFindById(account_set_id))?;
                let member_set = sets
                    .remove(&id)
                    .ok_or(AccountSetError::CouldNotFindById(id))?;

                if target.values().journal_id != member_set.values().journal_id {
                    return Err(AccountSetError::JournalIdMismatch);
                }

                (target, AccountId::from(id))
            }
        };

        self.assert_member_history_empty_in_op(
            op,
            account_set_id,
            account_set.values().journal_id,
            AccountId::from(&account_set.id()),
            member_id,
        )
        .await?;

        match member {
            AccountSetMemberId::Account(id) => {
                self.repo
                    .remove_member_account(op, account_set_id, id)
                    .await?;
            }
            AccountSetMemberId::AccountSet(id) => {
                self.repo.remove_member_set(op, account_set_id, id).await?;
            }
        }

        Ok(account_set)
    }

    #[instrument(level = "debug", name = "cala_ledger.account_sets.find_all", skip(self, account_set_ids), fields(account_set_ids_count = account_set_ids.len()))]
    pub async fn find_all<T: From<AccountSet>>(
        &self,
        account_set_ids: &[AccountSetId],
    ) -> Result<HashMap<AccountSetId, T>, AccountSetError> {
        Ok(self.repo.find_all(account_set_ids).await?)
    }

    #[instrument(level = "debug", name = "cala_ledger.account_sets.find_all_in_op", skip(self, op, account_set_ids), fields(account_set_ids_count = account_set_ids.len()))]
    pub async fn find_all_in_op<T: From<AccountSet>>(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        account_set_ids: &[AccountSetId],
    ) -> Result<HashMap<AccountSetId, T>, AccountSetError> {
        Ok(self.repo.find_all_in_op(op, account_set_ids).await?)
    }

    #[instrument(level = "debug", name = "cala_ledger.account_sets.find", skip(self))]
    pub async fn find(&self, account_set_id: AccountSetId) -> Result<AccountSet, AccountSetError> {
        Ok(self.repo.find_by_id(account_set_id).await?)
    }

    #[instrument(
        level = "debug",
        name = "cala_ledger.account_sets.find_in_op",
        skip(self, op)
    )]
    pub async fn find_in_op(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        account_set_id: AccountSetId,
    ) -> Result<AccountSet, AccountSetError> {
        Ok(self.repo.find_by_id_in_op(op, account_set_id).await?)
    }

    #[instrument(
        level = "debug",
        name = "cala_ledger.accounts_sets.find_by_external_id",
        skip(self)
    )]
    pub async fn find_by_external_id(
        &self,
        external_id: String,
    ) -> Result<AccountSet, AccountSetError> {
        Ok(self.repo.find_by_external_id(Some(external_id)).await?)
    }

    #[instrument(
        level = "debug",
        name = "cala_ledger.account_sets.find_where_member",
        skip(self)
    )]
    pub async fn find_where_member(
        &self,
        member: impl Into<AccountSetMemberId> + std::fmt::Debug,
        query: es_entity::PaginatedQueryArgs<AccountSetByNameCursor>,
    ) -> Result<es_entity::PaginatedQueryRet<AccountSet, AccountSetByNameCursor>, AccountSetError>
    {
        match member.into() {
            AccountSetMemberId::Account(account_id) => {
                self.repo
                    .find_where_account_is_member(account_id, query)
                    .await
            }
            AccountSetMemberId::AccountSet(account_set_id) => {
                self.repo
                    .find_where_account_set_is_member(account_set_id, query)
                    .await
            }
        }
    }

    #[instrument(
        level = "debug",
        name = "cala_ledger.account_sets.list_for_name",
        skip(self)
    )]
    pub async fn list_for_name(
        &self,
        name: String,
        args: es_entity::PaginatedQueryArgs<AccountSetByCreatedAtCursor>,
    ) -> Result<
        es_entity::PaginatedQueryRet<AccountSet, AccountSetByCreatedAtCursor>,
        AccountSetError,
    > {
        Ok(self
            .repo
            .list_for_name_by_created_at(name, args, Default::default())
            .await?)
    }

    #[instrument(
        level = "debug",
        name = "cala_ledger.account_sets.list_for_name_in_op",
        skip(self, op)
    )]
    pub async fn list_for_name_in_op(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        name: String,
        args: es_entity::PaginatedQueryArgs<AccountSetByCreatedAtCursor>,
    ) -> Result<
        es_entity::PaginatedQueryRet<AccountSet, AccountSetByCreatedAtCursor>,
        AccountSetError,
    > {
        Ok(self
            .repo
            .list_for_name_by_created_at_in_op(op, name, args, Default::default())
            .await?)
    }

    #[instrument(
        level = "debug",
        name = "cala_ledger.account_sets.find_where_member_in_op",
        skip(self, op)
    )]
    pub async fn find_where_member_in_op(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        member: impl Into<AccountSetMemberId> + std::fmt::Debug,
        query: es_entity::PaginatedQueryArgs<AccountSetByNameCursor>,
    ) -> Result<es_entity::PaginatedQueryRet<AccountSet, AccountSetByNameCursor>, AccountSetError>
    {
        match member.into() {
            AccountSetMemberId::Account(account_id) => {
                self.repo
                    .find_where_account_is_member_in_op(op, account_id, query)
                    .await
            }
            AccountSetMemberId::AccountSet(account_set_id) => {
                self.repo
                    .find_where_account_set_is_member_in_op(op, account_set_id, query)
                    .await
            }
        }
    }

    pub async fn list_members_by_created_at(
        &self,
        id: AccountSetId,
        args: es_entity::PaginatedQueryArgs<AccountSetMemberByCreatedAtCursor>,
    ) -> Result<
        es_entity::PaginatedQueryRet<AccountSetMember, AccountSetMemberByCreatedAtCursor>,
        AccountSetError,
    > {
        self.repo.list_children_by_created_at(id, args).await
    }

    pub async fn list_members_by_created_at_in_op(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        id: AccountSetId,
        args: es_entity::PaginatedQueryArgs<AccountSetMemberByCreatedAtCursor>,
    ) -> Result<
        es_entity::PaginatedQueryRet<AccountSetMember, AccountSetMemberByCreatedAtCursor>,
        AccountSetError,
    > {
        self.repo
            .list_children_by_created_at_in_op(op, id, args)
            .await
    }

    pub async fn list_members_by_external_id(
        &self,
        id: AccountSetId,
        args: es_entity::PaginatedQueryArgs<AccountSetMemberByExternalIdCursor>,
    ) -> Result<
        es_entity::PaginatedQueryRet<
            AccountSetMemberByExternalId,
            AccountSetMemberByExternalIdCursor,
        >,
        AccountSetError,
    > {
        self.repo.list_children_by_external_id(id, args).await
    }

    pub async fn list_members_by_external_id_in_op(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        id: AccountSetId,
        args: es_entity::PaginatedQueryArgs<AccountSetMemberByExternalIdCursor>,
    ) -> Result<
        es_entity::PaginatedQueryRet<
            AccountSetMemberByExternalId,
            AccountSetMemberByExternalIdCursor,
        >,
        AccountSetError,
    > {
        self.repo
            .list_children_by_external_id_in_op(op, id, args)
            .await
    }

    #[instrument(name = "cala_ledger.account_sets.recalculate_balances", skip(self))]
    pub async fn recalculate_balances(
        &self,
        account_set_id: AccountSetId,
    ) -> Result<(), AccountSetError> {
        let mut op = self.repo.begin_op_with_clock(&self.clock).await?;
        self.recalculate_balances_in_op(&mut op, account_set_id)
            .await?;
        op.commit().await?;
        Ok(())
    }

    #[instrument(
        name = "cala_ledger.account_sets.recalculate_balances_in_op",
        skip(self, op)
    )]
    pub async fn recalculate_balances_in_op(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        account_set_id: AccountSetId,
    ) -> Result<(), AccountSetError> {
        self.recalculate_balances_batch_in_op(op, &[account_set_id])
            .await
    }

    #[instrument(
        name = "cala_ledger.account_sets.recalculate_balances_batch",
        skip(self, account_set_ids),
        fields(account_set_ids_count = account_set_ids.len())
    )]
    pub async fn recalculate_balances_batch(
        &self,
        account_set_ids: &[AccountSetId],
    ) -> Result<(), AccountSetError> {
        let mut op = self.repo.begin_op_with_clock(&self.clock).await?;
        self.recalculate_balances_batch_in_op(&mut op, account_set_ids)
            .await?;
        op.commit().await?;
        Ok(())
    }

    #[instrument(
        name = "cala_ledger.account_sets.recalculate_balances_batch_in_op",
        skip(self, op, account_set_ids),
        fields(account_set_ids_count = account_set_ids.len())
    )]
    pub async fn recalculate_balances_batch_in_op(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        account_set_ids: &[AccountSetId],
    ) -> Result<(), AccountSetError> {
        if account_set_ids.is_empty() {
            return Ok(());
        }

        let sets = self
            .repo
            .find_all_in_op::<AccountSet>(&mut *op, account_set_ids)
            .await?;

        // Recalc is only meaningful for eventually-consistent account sets
        // (non-EC sets are maintained inline by posters and recalculating
        // them would race with within-batch `nextval` ordering on the
        // watermark). Reject any non-EC input up front so callers fail
        // loudly instead of silently risking a double-count.
        let account_ids: Vec<AccountId> = account_set_ids.iter().map(AccountId::from).collect();
        let accounts = self
            .accounts
            .find_all_in_op::<Account>(&mut *op, &account_ids)
            .await?;

        let mut journal_id: Option<JournalId> = None;
        for id in account_set_ids {
            let set = sets.get(id).ok_or(AccountSetError::CouldNotFindById(*id))?;
            let jid = set.values().journal_id;
            if let Some(expected) = journal_id {
                if jid != expected {
                    return Err(AccountSetError::JournalIdMismatch);
                }
            } else {
                journal_id = Some(jid);
            }

            let account = accounts
                .get(&AccountId::from(id))
                .ok_or(AccountSetError::CouldNotFindById(*id))?;
            if !account.values().config.eventually_consistent {
                return Err(AccountSetError::CannotRecalculateNonEcSet {
                    account_set_id: *id,
                });
            }
        }

        let journal_id = journal_id.expect("account_set_ids is non-empty");
        self.balances
            .recalculate_account_set_balances_batch_in_op(op, journal_id, account_set_ids)
            .await?;
        Ok(())
    }

    /// Recalculate balances for the given account sets **and** all their
    /// descendant account sets in a single batch.
    #[instrument(
        name = "cala_ledger.account_sets.recalculate_balances_deep",
        skip(self, account_set_ids),
        fields(account_set_ids_count = account_set_ids.len())
    )]
    pub async fn recalculate_balances_deep(
        &self,
        account_set_ids: &[AccountSetId],
    ) -> Result<(), AccountSetError> {
        let mut op = self.repo.begin_op_with_clock(&self.clock).await?;
        self.recalculate_balances_deep_in_op(&mut op, account_set_ids)
            .await?;
        op.commit().await?;
        Ok(())
    }

    #[instrument(
        name = "cala_ledger.account_sets.recalculate_balances_deep_in_op",
        skip(self, op, account_set_ids),
        fields(account_set_ids_count = account_set_ids.len())
    )]
    pub async fn recalculate_balances_deep_in_op(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        account_set_ids: &[AccountSetId],
    ) -> Result<(), AccountSetError> {
        if account_set_ids.is_empty() {
            return Ok(());
        }

        // Only walk EC descendants — non-EC descendants are maintained
        // inline by posters and recalc on them is rejected by
        // `recalculate_balances_batch_in_op`. Filtering them out here
        // means a deep walk on a hierarchy that mixes EC and non-EC sets
        // simply skips the non-EC nodes, instead of erroring.
        let descendants = self
            .repo
            .find_all_ec_descendant_set_ids(&mut *op, account_set_ids)
            .await?;

        let mut seen: std::collections::HashSet<AccountSetId> =
            account_set_ids.iter().copied().collect();
        let mut all_ids: Vec<AccountSetId> = account_set_ids.to_vec();
        for id in descendants {
            if seen.insert(id) {
                all_ids.push(id);
            }
        }

        self.recalculate_balances_batch_in_op(op, &all_ids).await
    }

    /// List the ids of all account sets that are marked as
    /// `eventually_consistent`.
    ///
    /// Intended as a building block for periodic reconciliation jobs that need
    /// to batch-recalculate balances for EC account sets (e.g. via
    /// [`Self::recalculate_balances_deep`]).
    #[instrument(
        level = "debug",
        name = "cala_ledger.account_sets.list_eventually_consistent_ids",
        skip(self)
    )]
    pub async fn list_eventually_consistent_ids(
        &self,
        args: es_entity::PaginatedQueryArgs<AccountSetByIdCursor>,
    ) -> Result<es_entity::PaginatedQueryRet<AccountSetId, AccountSetByIdCursor>, AccountSetError>
    {
        self.repo.list_eventually_consistent_ids(args).await
    }

    #[instrument(
        level = "debug",
        name = "cala_ledger.account_sets.list_eventually_consistent_ids_in_op",
        skip(self, op)
    )]
    pub async fn list_eventually_consistent_ids_in_op(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        args: es_entity::PaginatedQueryArgs<AccountSetByIdCursor>,
    ) -> Result<es_entity::PaginatedQueryRet<AccountSetId, AccountSetByIdCursor>, AccountSetError>
    {
        self.repo
            .list_eventually_consistent_ids_in_op(op, args)
            .await
    }

    /// Building block for a host-scheduled fill job: materialize pending
    /// transitive membership rows (ancestors of newly attached accounts
    /// and set edges) in bulk. Direct attaches write only the direct row;
    /// while a membership is pending, postings fall back to a live walk.
    /// Call repeatedly until it returns 0. Returns the number of
    /// memberships processed in this call.
    #[instrument(
        level = "debug",
        name = "cala_ledger.account_sets.fill_pending_transitive_memberships",
        skip(self),
        err(level = "warn")
    )]
    pub async fn fill_pending_transitive_memberships(
        &self,
        limit: usize,
    ) -> Result<usize, AccountSetError> {
        let mut op = self.repo.begin_op_with_clock(&self.clock).await?;
        let n = self
            .repo
            .fill_pending_transitive_memberships_in_op(&mut op, limit as i64)
            .await?;
        op.commit().await?;
        Ok(n)
    }

    pub(crate) async fn fetch_mappings_in_op(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        journal_id: JournalId,
        account_ids: &[AccountId],
    ) -> Result<HashMap<AccountId, Vec<AccountSetId>>, AccountSetError> {
        let rows = sqlx::query!(
            r#"
          SELECT m.account_set_id AS "set_id!: AccountSetId",
                 m.member_account_id AS "account_id!: AccountId",
                 m.transitive,
                 m.transitive_complete
          FROM cala_account_set_member_accounts m
          JOIN cala_account_sets s
          ON m.account_set_id = s.id AND s.journal_id = $1
          WHERE m.member_account_id = ANY($2)
          "#,
            journal_id as JournalId,
            account_ids as &[AccountId]
        )
        .fetch_all(op.as_executor())
        .await?;
        let mut mappings: HashMap<AccountId, Vec<AccountSetId>> = HashMap::new();
        let mut incomplete: Vec<(AccountId, AccountSetId)> = Vec::new();
        for row in rows {
            mappings.entry(row.account_id).or_default().push(row.set_id);
            if !row.transitive && !row.transitive_complete {
                incomplete.push((row.account_id, row.set_id));
            }
        }
        if !incomplete.is_empty() {
            // The async fill job hasn't materialized these memberships'
            // ancestor rows yet — walk the live hierarchy so postings
            // always see the complete set list (locks, velocity, rollup).
            let pair_account_ids: Vec<AccountId> =
                incomplete.iter().map(|(a, _)| *a).collect();
            let pair_set_ids: Vec<AccountSetId> = incomplete.iter().map(|(_, s)| *s).collect();
            let walk_rows = sqlx::query!(
                r#"
              WITH RECURSIVE input_pairs AS (
                SELECT * FROM UNNEST($1::uuid[], $2::uuid[]) AS v(account_id, account_set_id)
              ),
              parents AS (
                SELECT i.account_id, m.member_account_set_id, m.account_set_id
                FROM input_pairs i
                JOIN cala_account_set_member_account_sets m
                    ON m.member_account_set_id = i.account_set_id
                UNION ALL
                SELECT p.account_id, p.member_account_set_id, m.account_set_id
                FROM parents p
                JOIN cala_account_set_member_account_sets m
                    ON p.account_set_id = m.member_account_set_id
              )
              SELECT DISTINCT account_id AS "account_id!: AccountId",
                              account_set_id AS "set_id!: AccountSetId"
              FROM parents
              "#,
                &pair_account_ids as &[AccountId],
                &pair_set_ids as &[AccountSetId],
            )
            .fetch_all(op.as_executor())
            .await?;
            for row in walk_rows {
                mappings.entry(row.account_id).or_default().push(row.set_id);
            }
        }
        for sets in mappings.values_mut() {
            sets.sort_unstable();
            sets.dedup();
        }
        Ok(mappings)
    }
}

impl From<&AccountSetEvent> for OutboxEventPayload {
    fn from(event: &AccountSetEvent) -> Self {
        match event {
            AccountSetEvent::Initialized {
                values: account_set,
            } => OutboxEventPayload::AccountSetCreated {
                account_set: account_set.clone(),
            },
            AccountSetEvent::Updated { values, fields } => OutboxEventPayload::AccountSetUpdated {
                account_set: values.clone(),
                fields: fields.clone(),
            },
        }
    }
}
