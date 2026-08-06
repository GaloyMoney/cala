//! # EC set balance maintenance
//!
//! Non-EC account-set balances are maintained **inline** by posters
//! (`update_balances_in_op`), synchronously in the posting transaction.
//! Eventually-consistent (EC) set balances are excluded from that path
//! (`find_for_update` filters `eventually_consistent = FALSE`) and are
//! instead maintained **asynchronously** by the streaming rollup job
//! ([`crate::ec_rollup`]), which folds each committed transaction's leaf
//! deltas into its ancestor EC sets. That single, ordered, `spawn_unique`
//! writer is the only maintainer of EC-set balances.
//!
//! Posters take a shared advisory lock (`EC_SET_LOCK_CLASS`) on every
//! account they touch — leaves and ancestors alike. Its load-bearing role
//! is the member side of the membership guard
//! (`member_has_balance_history_in_op`): adding or removing an EC-set
//! member takes an EXCLUSIVE lock on that member, so a concurrent poster's
//! SHARED lock on the same member blocks until the guard's history check
//! has committed. That keeps a member from ever joining or leaving a set
//! while it has balance history, which is what makes EC sets
//! incremental-from-birth for the streaming rollup.

mod account_balance;
mod cursor;
mod effective;
pub mod error;
mod repo;
mod snapshot;

use chrono::{DateTime, NaiveDate, Utc};
use sqlx::PgPool;
use std::collections::{HashMap, HashSet};
use tracing::instrument;

pub use cala_types::{
    balance::{BalanceAmount, BalanceSnapshot},
    journal::JournalValues,
};
use cala_types::{entry::EntryValues, primitives::*};

use crate::{journal::Journals, outbox::*, primitives::JournalId};

pub use account_balance::*;
pub use cursor::*;
use effective::*;
use error::BalanceError;
use repo::*;
pub(crate) use snapshot::*;

/// One committed transaction's contribution to a streaming-rollup batch
/// (see [`Balances::apply_ec_rollup_in_op`]).
pub(crate) struct EcRollupTxn {
    pub journal_id: JournalId,
    pub effective: NaiveDate,
    pub created_at: DateTime<Utc>,
    pub entries: Vec<EntryValues>,
}

#[derive(Clone)]
pub struct Balances {
    repo: BalanceRepo,
    journals: Journals,
    effective: EffectiveBalances,
    _pool: PgPool,
}

impl Balances {
    pub(crate) fn new(pool: &PgPool, publisher: &OutboxPublisher, journals: &Journals) -> Self {
        Self {
            repo: BalanceRepo::new(pool),
            effective: EffectiveBalances::new(pool, publisher),
            journals: journals.clone(),
            _pool: pool.clone(),
        }
    }

    pub fn effective(&self) -> &EffectiveBalances {
        &self.effective
    }

    #[instrument(level = "debug", name = "cala_ledger.balance.find", skip(self))]
    pub async fn find(
        &self,
        journal_id: JournalId,
        account_id: impl Into<AccountId> + std::fmt::Debug,
        currency: Currency,
    ) -> Result<AccountBalance, BalanceError> {
        self.repo
            .find(journal_id, account_id.into(), currency)
            .await
    }

    #[instrument(
        level = "debug",
        name = "cala_ledger.balance.find_in_op",
        skip(self, op)
    )]
    pub async fn find_in_op(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        journal_id: JournalId,
        account_id: impl Into<AccountId> + std::fmt::Debug,
        currency: Currency,
    ) -> Result<AccountBalance, BalanceError> {
        self.repo
            .find_in_op(op, journal_id, account_id.into(), currency)
            .await
    }

    #[instrument(level = "debug", name = "cala_ledger.balance.find_all", skip(self, ids), fields(ids_count = ids.len()))]
    pub async fn find_all(
        &self,
        ids: &[BalanceId],
    ) -> Result<HashMap<BalanceId, AccountBalance>, BalanceError> {
        self.repo.find_all(ids).await
    }

    #[instrument(
        level = "debug",
        name = "cala_ledger.balance.list_for_account",
        skip(self)
    )]
    pub async fn list_for_account(
        &self,
        journal_id: JournalId,
        account_id: impl Into<AccountId> + std::fmt::Debug,
        args: es_entity::PaginatedQueryArgs<AccountBalanceByCurrencyCursor>,
    ) -> Result<
        es_entity::PaginatedQueryRet<AccountBalance, AccountBalanceByCurrencyCursor>,
        BalanceError,
    > {
        self.repo
            .list_for_account(journal_id, account_id.into(), args)
            .await
    }

    #[instrument(level = "debug", name = "cala_ledger.balance.list_for_accounts", skip(self, account_ids), fields(account_ids_count = account_ids.len()))]
    pub async fn list_for_accounts(
        &self,
        journal_id: JournalId,
        account_ids: &[AccountId],
        args: es_entity::PaginatedQueryArgs<AccountBalanceCursor>,
    ) -> Result<es_entity::PaginatedQueryRet<AccountBalance, AccountBalanceCursor>, BalanceError>
    {
        self.repo
            .list_for_accounts(journal_id, account_ids, args)
            .await
    }

    #[instrument(level = "debug", name = "cala_ledger.balance.find_all_in_op", skip(self, op, ids), fields(ids_count = ids.len()))]
    pub async fn find_all_in_op(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        ids: &[BalanceId],
    ) -> Result<HashMap<BalanceId, AccountBalance>, BalanceError> {
        self.repo.find_all_in_op(op, ids).await
    }

    #[instrument(
        level = "debug",
        name = "cala_ledger.balance.list_for_account_in_op",
        skip(self, op)
    )]
    pub async fn list_for_account_in_op(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        journal_id: JournalId,
        account_id: impl Into<AccountId> + std::fmt::Debug,
        args: es_entity::PaginatedQueryArgs<AccountBalanceByCurrencyCursor>,
    ) -> Result<
        es_entity::PaginatedQueryRet<AccountBalance, AccountBalanceByCurrencyCursor>,
        BalanceError,
    > {
        self.repo
            .list_for_account_in_op(op, journal_id, account_id.into(), args)
            .await
    }

    #[instrument(level = "debug", name = "cala_ledger.balance.list_for_accounts_in_op", skip(self, op, account_ids), fields(account_ids_count = account_ids.len()))]
    pub async fn list_for_accounts_in_op(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        journal_id: JournalId,
        account_ids: &[AccountId],
        args: es_entity::PaginatedQueryArgs<AccountBalanceCursor>,
    ) -> Result<es_entity::PaginatedQueryRet<AccountBalance, AccountBalanceCursor>, BalanceError>
    {
        self.repo
            .list_for_accounts_in_op(op, journal_id, account_ids, args)
            .await
    }

    #[instrument(
        level = "debug",
        name = "cala_ledger.balance.update_balances_in_op",
        skip(self, op, entries, account_set_mappings),
        fields(journal_id = %journal_id, entries_count = entries.len()),
        err(level = "warn")
    )]
    pub(crate) async fn update_balances_in_op(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        journal_id: JournalId,
        entries: Vec<EntryValues>,
        effective: NaiveDate,
        created_at: DateTime<Utc>,
        account_set_mappings: HashMap<AccountId, Vec<AccountSetId>>,
    ) -> Result<(), BalanceError> {
        let journal = self.journals.find(journal_id).await?;
        if journal.is_locked() {
            return Err(BalanceError::JournalLocked(journal.id));
        }

        let mut all_involved_balances: HashSet<_> = HashSet::new();
        let empty = Vec::new();
        for entry in entries.iter() {
            all_involved_balances.extend(
                account_set_mappings
                    .get(&entry.account_id)
                    .unwrap_or(&empty)
                    .iter()
                    .map(AccountId::from)
                    .chain(std::iter::once(entry.account_id))
                    .map(|id| (id, entry.currency)),
            );
        }

        let all_involved_balances: (Vec<_>, Vec<_>) = all_involved_balances
            .into_iter()
            .map(|(a, c)| (a, c.code()))
            .unzip();

        let current_balances = self
            .repo
            .find_for_update(op, journal.id, &all_involved_balances)
            .await?;
        let new_balances = Snapshots::from_entries(
            created_at,
            current_balances,
            &entries,
            &account_set_mappings,
        );
        self.repo
            .insert_new_snapshots(op, journal.id, new_balances)
            .await?;

        if journal.insert_effective_balances() {
            self.effective
                .update_cumulative_balances_in_op(
                    op,
                    journal_id,
                    entries,
                    effective,
                    created_at,
                    account_set_mappings,
                    all_involved_balances,
                )
                .await?;
        }

        Ok(())
    }

    /// Return `true` iff `member_id` has any row in
    /// `cala_balance_history` for `journal_id`, under the lock prelude
    /// described on `BalanceRepo::member_has_balance_history_in_op`.
    #[instrument(
        level = "debug",
        name = "cala_ledger.balance.member_has_balance_history_in_op",
        skip(self, op),
        fields(
            journal_id = %journal_id,
            parent_account_id = %parent_account_id,
            member_id = %member_id,
        ),
        err(level = "warn")
    )]
    pub(crate) async fn member_has_balance_history_in_op(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        journal_id: JournalId,
        parent_account_id: AccountId,
        member_id: AccountId,
    ) -> Result<bool, BalanceError> {
        self.repo
            .member_has_balance_history_in_op(op, journal_id, parent_account_id, member_id)
            .await
    }

    /// Batch variant of [`member_has_balance_history_in_op`](Self::member_has_balance_history_in_op):
    /// returns every member of `pairs` (`(journal_id, parent_account_id,
    /// member_id)`) that already has balance history in its journal.
    #[instrument(
        level = "debug",
        name = "cala_ledger.balance.members_with_balance_history_in_op",
        skip(self, op, pairs),
        fields(count = pairs.len()),
        err(level = "warn")
    )]
    pub(crate) async fn members_with_balance_history_in_op(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        pairs: &[(JournalId, AccountId, AccountId)],
    ) -> Result<Vec<AccountId>, BalanceError> {
        self.repo
            .members_with_balance_history_in_op(op, pairs)
            .await
    }

    /// Streaming EC rollup for a batch of committed transactions.
    ///
    /// Mirror of [`Self::update_balances_in_op`] but for the ancestor
    /// **eventually-consistent** account sets — the ones the inline poster
    /// path deliberately excludes. Folds every transaction's entry deltas
    /// into its EC ancestor sets (settled + effective), under the shared
    /// EC-set advisory lock. Caller drives this per outbox batch and owns
    /// the commit/checkpoint (see [`crate::ec_rollup`]).
    ///
    /// The settled pass is batched per journal: one mapping fetch, one
    /// sorted lock+read pass over the union of involved (set, currency)
    /// pairs — a single canonical lock acquisition per group rather than
    /// one per transaction — and one snapshot insert; the per-transaction
    /// folds chain in memory. The effective pass stays per transaction:
    /// its reads are effective-date-dependent (back-dating replay), so
    /// batching it would mean replaying across dates in memory.
    #[instrument(
        level = "debug",
        name = "cala_ledger.balance.apply_ec_rollup_in_op",
        skip_all,
        fields(txns_count = txns.len()),
        err(level = "warn")
    )]
    pub(crate) async fn apply_ec_rollup_in_op(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        txns: Vec<EcRollupTxn>,
    ) -> Result<(), BalanceError> {
        let mut groups: Vec<(JournalId, Vec<EcRollupTxn>)> = Vec::new();
        for tx in txns {
            match groups.iter_mut().find(|(j, _)| *j == tx.journal_id) {
                Some((_, group)) => group.push(tx),
                None => groups.push((tx.journal_id, vec![tx])),
            }
        }
        for (journal_id, group) in groups {
            self.apply_ec_rollup_group_in_op(op, journal_id, group)
                .await?;
        }
        Ok(())
    }

    async fn apply_ec_rollup_group_in_op(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        journal_id: JournalId,
        group: Vec<EcRollupTxn>,
    ) -> Result<(), BalanceError> {
        let member_account_ids: Vec<AccountId> = group
            .iter()
            .flat_map(|tx| tx.entries.iter().map(|e| e.account_id))
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();

        let ec_mappings = self
            .repo
            .fetch_ec_set_mappings(op, journal_id, &member_account_ids)
            .await?;
        if ec_mappings.is_empty() {
            return Ok(());
        }

        // Distinct (EC set-account, currency) pairs touched by the group.
        // Iteration order is not load-bearing: lock-acquisition ordering is
        // `find_ec_balances_for_update`'s concern (SQL-side `ORDER BY`).
        let empty = Vec::new();
        let mut involved: HashSet<(AccountId, Currency)> = HashSet::new();
        for entry in group.iter().flat_map(|tx| tx.entries.iter()) {
            for set_id in ec_mappings.get(&entry.account_id).unwrap_or(&empty) {
                involved.insert((AccountId::from(set_id), entry.currency));
            }
        }
        if involved.is_empty() {
            return Ok(());
        }
        let (account_ids, currencies): (Vec<AccountId>, Vec<&str>) =
            involved.into_iter().map(|(a, c)| (a, c.code())).unzip();

        // One sorted shared-lock acquisition + one read for the whole group.
        let mut current_balances = self
            .repo
            .find_ec_balances_for_update(op, journal_id, &(account_ids, currencies))
            .await?;

        // Chain the per-transaction folds in landing order: each fold reads
        // the map as left by its predecessors, so versions increment across
        // transactions exactly as they would with per-transaction reads.
        let mut all_new = Vec::new();
        for tx in group.iter() {
            let new_balances = Snapshots::from_ec_entries(
                tx.created_at,
                current_balances.clone(),
                &tx.entries,
                &ec_mappings,
            );
            for snapshot in new_balances.iter() {
                // Per pair the highest version lands last, so last-write-wins
                // leaves the map at each pair's latest snapshot.
                current_balances.insert(
                    (snapshot.account_id, snapshot.currency),
                    Some(snapshot.clone()),
                );
            }
            all_new.extend(new_balances);
        }
        if !all_new.is_empty() {
            self.repo
                .insert_new_snapshots(op, journal_id, all_new)
                .await?;
        }

        let journal = self.journals.find(journal_id).await?;
        if journal.insert_effective_balances() {
            for tx in group {
                // Keep the effective read narrow: only the pairs this
                // transaction touches.
                let mut tx_involved: HashSet<(AccountId, Currency)> = HashSet::new();
                for entry in tx.entries.iter() {
                    for set_id in ec_mappings.get(&entry.account_id).unwrap_or(&empty) {
                        tx_involved.insert((AccountId::from(set_id), entry.currency));
                    }
                }
                if tx_involved.is_empty() {
                    continue;
                }
                let (account_ids, currencies): (Vec<AccountId>, Vec<&str>) =
                    tx_involved.into_iter().map(|(a, c)| (a, c.code())).unzip();
                self.effective
                    .apply_ec_rollup_in_op(
                        op,
                        journal_id,
                        tx.entries,
                        tx.effective,
                        tx.created_at,
                        ec_mappings.clone(),
                        (account_ids, currencies),
                    )
                    .await?;
            }
        }

        Ok(())
    }
}
