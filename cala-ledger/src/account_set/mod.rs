mod entity;
pub mod error;
mod graph_cache;
mod graph_validation;
mod repo;

use es_entity::clock::ClockHandle;
use sqlx::PgPool;
use std::collections::HashMap;
use tracing::instrument;

use crate::{
    account::*, account_set_member::AccountSetMembers, balance::*, outbox::*, primitives::JournalId,
};

pub use crate::account_set_member::members_cursor::*;
pub use entity::*;
use error::*;
use graph_cache::SetGraphCache;
/// Re-exported for the posting flow, which reads the direct-membership probe
/// seeds as part of its single read statement and hands them back to the
/// set-graph cache.
pub(crate) use graph_validation::AccountMembership;
use graph_validation::SetMembership;
pub use repo::account_set_cursor::*;
use repo::*;

#[derive(Clone)]
pub struct AccountSets {
    repo: AccountSetRepo,
    accounts: Accounts,
    balances: Balances,
    account_set_members: AccountSetMembers,
    /// Internal detail of this module: the epoch-validated in-process
    /// cache backing the posting hot path's ancestor resolution
    /// (`fetch_mappings_in_op`). Holds its own handle on the repo — all
    /// SQL stays in `repo.rs`; the cache orchestrates. Shared across
    /// clones.
    set_graph_cache: SetGraphCache,
    clock: ClockHandle,
}

impl AccountSets {
    pub(crate) fn new(
        pool: &PgPool,
        publisher: &OutboxPublisher,
        accounts: &Accounts,
        balances: &Balances,
        account_set_members: &AccountSetMembers,
        clock: &ClockHandle,
    ) -> Self {
        let repo = AccountSetRepo::new(pool, publisher);
        Self {
            set_graph_cache: SetGraphCache::new(repo.clone()),
            repo,
            accounts: accounts.clone(),
            balances: balances.clone(),
            account_set_members: account_set_members.clone(),
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
            member_id,
        )
        .await?;

        match member {
            AccountSetMemberId::Account(id) => {
                self.repo.lock_graph_shared_in_op(&mut *op).await?;
                self.account_set_members
                    .lock_members_in_op(&mut *op, &[id])
                    .await?;
                self.set_graph_cache
                    .assert_no_double_membership_in_op(
                        op,
                        &[AccountMembership {
                            account_set_id,
                            account_id: id,
                        }],
                    )
                    .await?;
                self.account_set_members
                    .add_in_op(&mut *op, &[(account_set_id, id)])
                    .await?;
            }
            AccountSetMemberId::AccountSet(id) => {
                let edge = SetMembership {
                    account_set_id,
                    member_account_set_id: id,
                };
                self.repo.lock_for_set_membership_op(op).await?;
                self.set_graph_cache
                    .assert_valid_set_memberships_in_op(op, &[edge])
                    .await?;
                self.repo.insert_member_sets(op, &[edge]).await?;
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
    /// memberships in a single statement, instead of one insert per
    /// account. Callers attaching many accounts at once should prefer
    /// this over looping `add_member_in_op`.
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

        // The public API takes plain pairs; convert once here so every layer
        // below the service names the two ends.
        let members: Vec<AccountMembership> = members
            .iter()
            .copied()
            .map(AccountMembership::from)
            .collect();

        let account_set_ids: Vec<AccountSetId> = members.iter().map(|m| m.account_set_id).collect();
        let sets = self
            .repo
            .find_all_in_op::<AccountSet>(&mut *op, &account_set_ids)
            .await?;

        let mut check_pairs = Vec::with_capacity(members.len());
        for membership in &members {
            let set = sets
                .get(&membership.account_set_id)
                .ok_or(AccountSetError::CouldNotFindById(membership.account_set_id))?;
            check_pairs.push((set.values().journal_id, membership.account_id));
        }
        let with_history = self
            .balances
            .members_with_balance_history_in_op(op, &check_pairs)
            .await?;
        if let Some(member_id) = with_history.into_iter().next() {
            let membership = members
                .iter()
                .find(|m| m.account_id == member_id)
                .expect("member with history must be in input");
            return Err(AccountSetError::MemberHasBalanceHistory {
                account_set_id: membership.account_set_id,
                member_id,
            });
        }

        let account_ids: Vec<AccountId> = members.iter().map(|m| m.account_id).collect();
        self.repo.lock_graph_shared_in_op(op).await?;
        self.account_set_members
            .lock_members_in_op(op, &account_ids)
            .await?;
        self.set_graph_cache
            .assert_no_double_membership_in_op(op, &members)
            .await?;
        let pairs: Vec<(AccountSetId, AccountId)> = members
            .iter()
            .map(|m| (m.account_set_id, m.account_id))
            .collect();
        self.account_set_members.add_in_op(op, &pairs).await?;

        Ok(())
    }

    #[instrument(
        level = "debug",
        name = "cala_ledger.account_sets.add_member_sets",
        skip(self, members),
        fields(count = members.len())
    )]
    pub async fn add_member_sets(
        &self,
        members: &[(AccountSetId, AccountSetId)],
    ) -> Result<(), AccountSetError> {
        let mut op = self.repo.begin_op_with_clock(&self.clock).await?;
        self.add_member_sets_in_op(&mut op, members).await?;
        op.commit().await?;
        Ok(())
    }

    /// Batch variant of [`add_member_in_op`](Self::add_member_in_op) for
    /// account-set hierarchy edges. The complete proposed graph is validated
    /// before any edge is inserted, then all direct edges, the epoch bump,
    /// and outbox events are persisted atomically.
    ///
    /// **Cost model.** The first consumer is a one-shot chart import that
    /// builds a fresh hierarchy, so existing edges and account memberships
    /// are near-empty and validation is dominated by the proposed batch
    /// itself. Against a mature chart the account read is scoped to the
    /// descendant closure of the proposed member endpoints, and existing
    /// edges are served from the epoch-validated in-process cache, so a
    /// small batch does not re-read or re-validate the whole graph. A
    /// single proposed edge (from here or from `add_member_in_op`) runs
    /// through the same combined-graph machinery: with a warm cache it
    /// validates in memory, and its SQL fallback is one flat edge read —
    /// bounded by the edge table. The exclusive membership-graph lock is
    /// held for the whole op, so very large batches will block other
    /// structure and account-member writers for the duration of
    /// validation and insert.
    #[instrument(
        level = "debug",
        name = "cala_ledger.account_sets.add_member_sets_in_op",
        skip(self, op, members),
        fields(count = members.len()),
        err(level = "warn")
    )]
    pub async fn add_member_sets_in_op(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        members: &[(AccountSetId, AccountSetId)],
    ) -> Result<(), AccountSetError> {
        if members.is_empty() {
            return Ok(());
        }

        // The public API takes plain pairs; convert once here so every layer
        // below the service names container and member explicitly.
        let members: Vec<SetMembership> =
            members.iter().copied().map(SetMembership::from).collect();

        let account_set_ids: Vec<AccountSetId> = {
            let mut ids: Vec<AccountSetId> = members
                .iter()
                .flat_map(|edge| [edge.account_set_id, edge.member_account_set_id])
                .collect();
            ids.sort_unstable();
            ids.dedup();
            ids
        };
        let sets = self
            .repo
            .find_all_in_op::<AccountSet>(&mut *op, &account_set_ids)
            .await?;

        let mut check_pairs = Vec::with_capacity(members.len());
        for edge in &members {
            let account_set = sets
                .get(&edge.account_set_id)
                .ok_or(AccountSetError::CouldNotFindById(edge.account_set_id))?;
            let member_account_set =
                sets.get(&edge.member_account_set_id)
                    .ok_or(AccountSetError::CouldNotFindById(
                        edge.member_account_set_id,
                    ))?;

            if account_set.values().journal_id != member_account_set.values().journal_id {
                return Err(AccountSetError::JournalIdMismatch);
            }

            check_pairs.push((
                account_set.values().journal_id,
                AccountId::from(edge.member_account_set_id),
            ));
        }

        let with_history = self
            .balances
            .members_with_balance_history_in_op(op, &check_pairs)
            .await?;
        if let Some(member_id) = with_history.into_iter().next() {
            let edge = members
                .iter()
                .find(|edge| AccountId::from(edge.member_account_set_id) == member_id)
                .expect("member with history must be in input");
            return Err(AccountSetError::MemberHasBalanceHistory {
                account_set_id: edge.account_set_id,
                member_id,
            });
        }

        self.repo.lock_for_set_membership_op(op).await?;
        self.set_graph_cache
            .assert_valid_set_memberships_in_op(op, &members)
            .await?;
        self.repo.insert_member_sets(op, &members).await?;

        Ok(())
    }

    /// `cala_balance_history` row in `journal_id`. Folding existing
    /// balance into a parent set after the fact is unsafe: the streaming
    /// rollup only folds in a member's activity from the point it joins,
    /// so pre-existing balance would be silently dropped, and the
    /// symmetric remove case has no safe unfold path either, so we forbid
    /// both.
    ///
    /// The check itself is run under an EXCLUSIVE lock on the candidate
    /// member in the EC-set lock namespace — the guard side of the
    /// attach fence (posters hold SHARED on their entry accounts from
    /// before the first entry insert) — so the existence query reflects
    /// committed state even with concurrent posters in flight.
    async fn assert_member_history_empty_in_op(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        account_set_id: AccountSetId,
        journal_id: JournalId,
        member_id: AccountId,
    ) -> Result<(), AccountSetError> {
        if self
            .balances
            .member_has_balance_history_in_op(op, journal_id, member_id)
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
            member_id,
        )
        .await?;

        match member {
            AccountSetMemberId::Account(id) => {
                self.repo.lock_graph_shared_in_op(op).await?;
                self.account_set_members
                    .lock_members_in_op(op, &[id])
                    .await?;
                self.account_set_members
                    .remove_in_op(op, account_set_id, id)
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
        Ok(self
            .account_set_members
            .list_by_created_at(id, args)
            .await?)
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
        Ok(self
            .account_set_members
            .list_by_created_at_in_op(op, id, args)
            .await?)
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
        Ok(self
            .account_set_members
            .list_by_external_id(id, args)
            .await?)
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
        Ok(self
            .account_set_members
            .list_by_external_id_in_op(op, id, args)
            .await?)
    }

    /// Resolve each entry account's ancestor-set mappings AND take the
    /// poster's per-balance locks on the non-EC ancestors, via the
    /// epoch-validated set-graph cache — see the `graph_cache` module docs for
    /// the resolution paths and lock doctrine.
    ///
    /// The membership probe itself is issued by the caller (the posting flow
    /// folds it into its single read statement), which is sound precisely
    /// because the probe takes no locks of its own.
    pub(crate) async fn resolve_mappings_from_probe_in_op(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        journal_id: JournalId,
        probe_epoch: i64,
        probe_seeds: &[AccountMembership],
        entry_pairs: &(Vec<AccountId>, Vec<&str>),
    ) -> Result<HashMap<AccountId, Vec<AccountSetId>>, AccountSetError> {
        self.set_graph_cache
            .resolve_from_probe_in_op(op, journal_id, probe_epoch, probe_seeds, entry_pairs)
            .await
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
