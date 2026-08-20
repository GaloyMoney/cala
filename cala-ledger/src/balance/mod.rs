//! # EC set balance maintenance
//!
//! Non-EC account-set balances are maintained **inline** by posters
//! (`update_balances_in_op`), synchronously in the posting transaction.
//! Eventually-consistent (EC) set balances are excluded from that path
//! (`find_for_update` filters `eventually_consistent = FALSE`) and are
//! instead maintained **asynchronously** by the streaming rollup job
//! ([`crate::ec_rollup`]), which folds each committed transaction's leaf
//! deltas into its ancestor EC sets. That single, ordered, resident-job
//! writer is the only maintainer of EC-set balances.
//!
//! Class-1 advisory-lock doctrine (`EC_SET_LOCK_CLASS`), in full:
//! SHARED = the poster, on its distinct **entry accounts** (leaves
//! only, EC and non-EC alike — never ancestors), held from *before its
//! first entry insert* to commit (`lock_entry_balances_in_op`);
//! EXCLUSIVE = the membership guard on the member being added/removed
//! (`member_has_balance_history_in_op`); the streaming rollup applier
//! takes SHARED on the EC accounts it writes
//! (`find_ec_balances_for_update`).
//!
//! That poster-SHARED vs guard-EXCLUSIVE pair is the attach fence, and
//! taking the SHARED side before the first entry insert (and before the
//! mappings read) makes it cover the poster's entire transaction: an
//! attach of an account with an in-flight first posting blocks until
//! the poster commits and is then correctly rejected (the guard checks
//! `cala_entries` as well as `cala_balance_history`, so EC activity —
//! whose history is only written later by the rollup — still counts),
//! while a poster that starts after an attach took its EXCLUSIVE blocks
//! *before* reading set mappings and resumes seeing the new membership.
//!
//! The create-inside-set fast path (`NewAccount::initial_account_set`)
//! is exempt from the fence: the account row is uncommitted so no
//! balance history can exist (the guard is vacuously satisfied), and no
//! posting to a not-yet-visible account can be in flight — from first
//! visibility its membership already exists.

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
    balance::{BalanceAmount, BalanceSnapshot, EffectiveBalanceSnapshot},
    journal::JournalValues,
};
use cala_types::{entry::EntryValues, primitives::*};

use crate::{journal::Journals, primitives::JournalId};

pub use account_balance::*;
pub use cursor::*;
#[cfg(feature = "fuzz")]
pub use effective::fuzz_recalculate;
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
    pub(crate) fn new(pool: &PgPool, journals: &Journals) -> Self {
        Self {
            repo: BalanceRepo::new(pool),
            effective: EffectiveBalances::new(pool),
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

    /// Return `true` iff `member_id` has any row in
    /// `cala_balance_history` for `journal_id`, under the lock prelude
    /// described on `BalanceRepo::member_has_balance_history_in_op`.
    #[instrument(
        level = "debug",
        name = "cala_ledger.balance.member_has_balance_history_in_op",
        skip(self, op),
        fields(
            journal_id = %journal_id,
            member_id = %member_id,
        ),
        err(level = "warn")
    )]
    pub(crate) async fn member_has_balance_history_in_op(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        journal_id: JournalId,
        member_id: AccountId,
    ) -> Result<bool, BalanceError> {
        self.repo
            .member_has_balance_history_in_op(op, journal_id, member_id)
            .await
    }

    /// Batch variant of [`member_has_balance_history_in_op`](Self::member_has_balance_history_in_op):
    /// returns every member of `pairs` (`(journal_id, member_id)`) that
    /// already has balance history in its journal.
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
        pairs: &[(JournalId, AccountId)],
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
        // EC leaves the inline poster skips, folded here into their own balance.
        let ec_leaves = self
            .repo
            .fetch_ec_leaf_accounts(op, &member_account_ids)
            .await?;
        if ec_mappings.is_empty() && ec_leaves.is_empty() {
            return Ok(());
        }

        let empty = Vec::new();
        let mut involved: HashSet<(AccountId, Currency)> = HashSet::new();
        for entry in group.iter().flat_map(|tx| tx.entries.iter()) {
            for set_id in ec_mappings.get(&entry.account_id).unwrap_or(&empty) {
                involved.insert((AccountId::from(set_id), entry.currency));
            }
            if ec_leaves.contains(&entry.account_id) {
                involved.insert((entry.account_id, entry.currency));
            }
        }
        if involved.is_empty() {
            return Ok(());
        }
        let (account_ids, currencies): (Vec<AccountId>, Vec<&str>) =
            involved.into_iter().map(|(a, c)| (a, c.code())).unzip();

        let mut current_balances = self
            .repo
            .find_ec_balances_for_update(op, journal_id, &(account_ids, currencies))
            .await?;

        let mut all_new = Vec::new();
        for tx in group.iter() {
            let new_balances = Snapshots::from_ec_entries(
                tx.created_at,
                current_balances.clone(),
                &tx.entries,
                &ec_mappings,
                &ec_leaves,
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

        let journal = self.journals.find_in_op(&mut *op, journal_id).await?;
        if journal.insert_effective_balances() {
            for tx in group {
                let mut tx_involved: HashSet<(AccountId, Currency)> = HashSet::new();
                for entry in tx.entries.iter() {
                    for set_id in ec_mappings.get(&entry.account_id).unwrap_or(&empty) {
                        tx_involved.insert((AccountId::from(set_id), entry.currency));
                    }
                    if ec_leaves.contains(&entry.account_id) {
                        tx_involved.insert((entry.account_id, entry.currency));
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
                        &ec_leaves,
                    )
                    .await?;
            }
        }

        Ok(())
    }
}
