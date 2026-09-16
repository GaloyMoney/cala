mod data;
mod repo;

use chrono::{DateTime, NaiveDate, Utc};
use sqlx::PgPool;
use std::collections::{HashMap, HashSet};
use tracing::instrument;

use cala_types::{balance::EffectiveBalanceSnapshot, entry::EntryValues, primitives::*};

use crate::primitives::JournalId;

use super::{
    account_balance::*,
    cursor::{
        AccountBalanceByCurrencyCursor, AccountBalanceCursor, EffectiveBalancesModifiedCursor,
    },
    error::BalanceError,
    EcRollupTxn,
};

use repo::*;

#[derive(Clone)]
pub struct EffectiveBalances {
    repo: EffectiveBalanceRepo,
    _pool: PgPool,
}
impl EffectiveBalances {
    pub(crate) fn new(pool: &PgPool) -> Self {
        Self {
            repo: EffectiveBalanceRepo::new(pool),
            _pool: pool.clone(),
        }
    }

    #[instrument(
        level = "debug",
        name = "cala_ledger.balance.effective.find_cumulative",
        skip(self)
    )]
    pub async fn find_cumulative(
        &self,
        journal_id: JournalId,
        account_id: impl Into<AccountId> + std::fmt::Debug,
        currency: Currency,
        date: NaiveDate,
    ) -> Result<AccountBalance, BalanceError> {
        self.repo
            .find(journal_id, account_id.into(), currency, date)
            .await
    }

    #[instrument(
        level = "debug",
        name = "cala_ledger.balance.effective.find_in_range",
        skip(self)
    )]
    pub async fn find_in_range(
        &self,
        journal_id: JournalId,
        account_id: AccountId,
        currency: Currency,
        from: NaiveDate,
        until: Option<NaiveDate>,
    ) -> Result<BalanceRange, BalanceError> {
        match self
            .repo
            .find_range(journal_id, account_id, currency, from, until)
            .await?
        {
            (start, Some(end), version_diff) => Ok(BalanceRange::new(start, end, version_diff)),
            _ => Err(BalanceError::NotFound(journal_id, account_id, currency)),
        }
    }

    #[instrument(level = "debug", name = "cala_ledger.balance.effective.find_all_cumulative", skip(self, ids), fields(ids_count = ids.len()))]
    pub async fn find_all_cumulative(
        &self,
        ids: &[BalanceId],
        date: NaiveDate,
    ) -> Result<HashMap<BalanceId, AccountBalance>, BalanceError> {
        self.repo.find_all(ids, date).await
    }

    #[instrument(
        level = "debug",
        name = "cala_ledger.balance.effective.list_cumulative_for_account",
        skip(self)
    )]
    pub async fn list_cumulative_for_account(
        &self,
        journal_id: JournalId,
        account_id: impl Into<AccountId> + std::fmt::Debug,
        date: NaiveDate,
        args: es_entity::PaginatedQueryArgs<AccountBalanceByCurrencyCursor>,
    ) -> Result<
        es_entity::PaginatedQueryRet<AccountBalance, AccountBalanceByCurrencyCursor>,
        BalanceError,
    > {
        self.repo
            .list_for_account(journal_id, account_id.into(), date, args)
            .await
    }

    #[instrument(
        level = "debug",
        name = "cala_ledger.balance.effective.list_cumulative_for_accounts",
        skip(self, account_ids),
        fields(account_ids_count = account_ids.len())
    )]
    pub async fn list_cumulative_for_accounts(
        &self,
        journal_id: JournalId,
        account_ids: &[AccountId],
        date: NaiveDate,
        args: es_entity::PaginatedQueryArgs<AccountBalanceCursor>,
    ) -> Result<es_entity::PaginatedQueryRet<AccountBalance, AccountBalanceCursor>, BalanceError>
    {
        self.repo
            .list_for_accounts(journal_id, account_ids, date, args)
            .await
    }

    #[instrument(level = "debug", name = "cala_ledger.balance.effective.find_all_in_range", skip(self, ids), fields(ids_count = ids.len()))]
    pub async fn find_all_in_range(
        &self,
        ids: &[BalanceId],
        from: NaiveDate,
        until: Option<NaiveDate>,
    ) -> Result<HashMap<BalanceId, BalanceRange>, BalanceError> {
        let ranges = self.repo.find_range_all(ids, from, until).await?;
        Ok(ranges
            .into_iter()
            .filter_map(|(id, (start, start_version, end, end_version))| {
                BalanceRange::from_bounds(start, start_version, end, end_version)
                    .map(|range| (id, range))
            })
            .collect())
    }

    #[instrument(
        level = "debug",
        name = "cala_ledger.balance.effective.list_in_range_for_account",
        skip(self)
    )]
    pub async fn list_in_range_for_account(
        &self,
        journal_id: JournalId,
        account_id: impl Into<AccountId> + std::fmt::Debug,
        from: NaiveDate,
        until: Option<NaiveDate>,
        args: es_entity::PaginatedQueryArgs<AccountBalanceByCurrencyCursor>,
    ) -> Result<
        es_entity::PaginatedQueryRet<BalanceRange, AccountBalanceByCurrencyCursor>,
        BalanceError,
    > {
        self.repo
            .list_range_for_account(journal_id, account_id.into(), from, until, args)
            .await
    }

    #[instrument(
        level = "debug",
        name = "cala_ledger.balance.effective.list_in_range_for_accounts",
        skip(self, account_ids),
        fields(account_ids_count = account_ids.len())
    )]
    pub async fn list_in_range_for_accounts(
        &self,
        journal_id: JournalId,
        account_ids: &[AccountId],
        from: NaiveDate,
        until: Option<NaiveDate>,
        args: es_entity::PaginatedQueryArgs<AccountBalanceCursor>,
    ) -> Result<es_entity::PaginatedQueryRet<BalanceRange, AccountBalanceCursor>, BalanceError>
    {
        self.repo
            .list_range_for_accounts(journal_id, account_ids, from, until, args)
            .await
    }

    /// Enumerate every `(account_id, currency, effective)` tuple under
    /// `journal_id` that has had a cumulative-effective-balance snapshot
    /// written since `since`, returning each tuple's overall-latest
    /// [`EffectiveBalanceSnapshot`] (not merely the latest one written since
    /// `since` — the tuple's newest row overall, so callers never need a
    /// second read). A CDC-style pull API: designed to replace entry-derived
    /// dirty-tracking side tables built against per-snapshot outbox events.
    ///
    /// # Contract (load-bearing — read before wiring up a consumer)
    ///
    /// 1. **Watermark race.** The watermark column (`modified_at`) is
    ///    assigned at INSERT time inside a transaction that may not commit
    ///    until later; a reader using `since = now()` can miss rows from
    ///    transactions that were still in flight at read time. Callers MUST
    ///    re-query with an overlap window (`since = previous_watermark -
    ///    overlap`, with `overlap` much greater than the expected max
    ///    transaction duration) and MUST be idempotent under re-delivery of
    ///    tuples that did not actually change. A global sequence with gap
    ///    tracking was considered and rejected as unwarranted machinery for
    ///    an EOD-cadence consumer.
    ///
    ///    Note this filters on `modified_at`, not `created_at`: `created_at`
    ///    is set once at row-genesis for an (account_id, currency) chain and
    ///    carried forward unchanged on every later row for that chain, so it
    ///    does not mark per-row write time — `modified_at` does, including
    ///    for backdating-rewritten rows.
    /// 2. **EC completeness.** Snapshots for eventually-consistent
    ///    accounts/sets are written only when the streaming EC rollup
    ///    flushes. This method reports what has been *written* — it does
    ///    not wait for anything. A caller wanting a complete picture as of a
    ///    moment must fence first via
    ///    [`CalaLedger::ec_rollup_status`](crate::CalaLedger::ec_rollup_status)
    ///    `.await_completion(..)`.
    /// 3. **Clock domain.** `modified_at` comes from the ledger's configured
    ///    clock. Prefer deriving `since` from previously *returned* data or
    ///    the caller's own job-state watermark rather than wall-clock
    ///    `now()` on the caller's side, which can diverge (e.g. under
    ///    simulated time).
    /// 4. Requires `enable_effective_balance = true` on the journal —
    ///    otherwise no snapshot rows exist and this returns empty pages,
    ///    not an error.
    #[instrument(
        level = "debug",
        name = "cala_ledger.balance.effective.list_modified_since",
        skip(self)
    )]
    pub async fn list_modified_since(
        &self,
        journal_id: JournalId,
        since: DateTime<Utc>,
        args: es_entity::PaginatedQueryArgs<EffectiveBalancesModifiedCursor>,
    ) -> Result<
        es_entity::PaginatedQueryRet<EffectiveBalanceSnapshot, EffectiveBalancesModifiedCursor>,
        BalanceError,
    > {
        self.repo.list_modified_since(journal_id, since, args).await
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn update_cumulative_balances_in_op(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        journal_id: JournalId,
        entries: Vec<EntryValues>,
        effective: NaiveDate,
        created_at: DateTime<Utc>,
        mappings: HashMap<AccountId, Vec<AccountSetId>>,
        balance_ids: (Vec<AccountId>, Vec<&str>),
    ) -> Result<(), BalanceError> {
        let mut all_data = self
            .repo
            .find_for_update(&mut *op, journal_id, balance_ids, effective)
            .await?;
        let empty = Vec::new();
        for entry in entries.iter() {
            for account_id in mappings
                .get(&entry.account_id)
                .unwrap_or(&empty)
                .iter()
                .map(AccountId::from)
                .chain(std::iter::once(entry.account_id))
            {
                if let Some(data) = all_data.get_mut(&(account_id, entry.currency)) {
                    data.push(effective, 0, created_at, entry);
                }
            }
        }
        for data in all_data.values_mut() {
            data.re_calculate_snapshots(created_at);
        }

        let new_balances = all_data
            .into_values()
            .flat_map(|data| data.into_snapshots(journal_id))
            .collect();
        self.repo
            .insert_new_snapshots(op, journal_id, new_balances)
            .await?;

        Ok(())
    }

    /// EC counterpart of [`Self::update_cumulative_balances_in_op`] used by
    /// the streaming rollup: fans every transaction's entries in the batch
    /// into their EC ancestor sets and, for an entry whose leaf is an EC
    /// plain account (listed in `ec_leaves`), into that leaf's own
    /// cumulative-effective balance too.
    ///
    /// Batched per pair rather than per transaction: each `(account,
    /// currency)` pair's later history is read **once**, anchored at the
    /// *earliest* effective date any of the batch's transactions gives it,
    /// and every one of the batch's entries for that pair is folded into
    /// the same in-memory replay before one insert. A backdated transaction
    /// therefore still rewrites every later row for the pairs it touches,
    /// but does so once per batch rather than once per transaction —
    /// `SnapshotOrEntry`'s ordering (pre-existing rows before the batch's
    /// own entries, then landing order) makes the fold equivalent to
    /// applying the batch's transactions one at a time.
    #[instrument(
        level = "debug",
        name = "cala_ledger.balance.effective.apply_ec_rollup_batch_in_op",
        skip_all,
        fields(txns_count = txns.len()),
        err(level = "warn")
    )]
    pub(crate) async fn apply_ec_rollup_batch_in_op(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        journal_id: JournalId,
        txns: &[EcRollupTxn<'_>],
        ec_mappings: &HashMap<AccountId, Vec<AccountSetId>>,
        ec_leaves: &HashSet<AccountId>,
    ) -> Result<(), BalanceError> {
        let targets = |account_id: &AccountId| {
            ec_mappings
                .get(account_id)
                .into_iter()
                .flatten()
                .map(AccountId::from)
                .chain(ec_leaves.get(account_id).copied())
        };

        // Each pair's later history only needs to be read from its
        // *earliest* effective date in the batch — reading from the global
        // minimum across all pairs would delete and rewrite rows for pairs
        // whose own entries are all later, for no benefit.
        let mut earliest: HashMap<(AccountId, Currency), NaiveDate> = HashMap::new();
        for tx in txns {
            for entry in tx.entries.iter().copied() {
                for target in targets(&entry.account_id) {
                    earliest
                        .entry((target, entry.currency))
                        .and_modify(|date| *date = (*date).min(tx.effective))
                        .or_insert(tx.effective);
                }
            }
        }
        if earliest.is_empty() {
            return Ok(());
        }

        // One `find_ec_for_update` call per distinct earliest date — in
        // practice a batch spans one or two dates, so this is one or two
        // queries instead of one per transaction.
        let mut by_date: HashMap<NaiveDate, (Vec<AccountId>, Vec<&str>)> = HashMap::new();
        for (&(account_id, currency), &date) in earliest.iter() {
            let (ids, currencies) = by_date.entry(date).or_default();
            ids.push(account_id);
            currencies.push(currency.code());
        }
        let mut all_data = HashMap::new();
        for (date, ids) in by_date {
            all_data.extend(
                self.repo
                    .find_ec_for_update(&mut *op, journal_id, ids, date)
                    .await?,
            );
        }

        for (tx_index, tx) in txns.iter().enumerate() {
            for entry in tx.entries.iter().copied() {
                for target in targets(&entry.account_id) {
                    if let Some(data) = all_data.get_mut(&(target, entry.currency)) {
                        data.push(tx.effective, tx_index, tx.created_at, entry);
                    }
                }
            }
        }

        let rewritten_at = txns
            .iter()
            .map(|tx| tx.created_at)
            .max()
            .expect("txns is non-empty: earliest was populated from it above");
        for data in all_data.values_mut() {
            data.re_calculate_snapshots(rewritten_at);
        }

        let new_balances = all_data
            .into_values()
            .flat_map(|data| data.into_snapshots(journal_id))
            .collect();
        self.repo
            .insert_new_snapshots(op, journal_id, new_balances)
            .await?;

        Ok(())
    }
}

#[cfg(feature = "fuzz")]
mod __fuzz {
    //! Harness for the out-of-tree `effective_balance` fuzz target. Lives in
    //! this module so it can reach the `pub(super)` `EffectiveBalanceData`.
    use super::data::{EffectiveBalanceData, SnapshotOrEntry};
    use cala_types::{
        balance::BalanceSnapshot,
        entry::EntryValues,
        primitives::{AccountId, Currency, JournalId},
    };
    use chrono::{NaiveDate, Utc};
    use serde::Deserialize;

    #[derive(Deserialize)]
    struct DatedSnapshot {
        effective: NaiveDate,
        values: BalanceSnapshot,
    }

    #[derive(Deserialize)]
    struct PlanOp {
        effective: NaiveDate,
        kind: String,
        idx: usize,
        /// Which transaction this op belongs to (landing order). Lets the
        /// fuzzer generate mixed-date, mixed-transaction batches — the
        /// shape `EffectiveBalanceData::re_calculate_snapshots` now folds
        /// in one pass instead of one call per transaction.
        #[serde(default)]
        tx_index: usize,
    }

    pub fn fuzz_recalculate(data: &[u8]) {
        let parts: Vec<&[u8]> = data.split(|&b| b == 0xFF).collect();
        if parts.len() < 4 {
            return;
        }
        let Ok(entries) = serde_json::from_slice::<Vec<EntryValues>>(parts[0]) else {
            return;
        };
        let Ok(snapshots) = serde_json::from_slice::<Vec<DatedSnapshot>>(parts[1]) else {
            return;
        };
        let last = serde_json::from_slice::<DatedSnapshot>(parts[2]).ok();
        let Ok(plan) = serde_json::from_slice::<Vec<PlanOp>>(parts[3]) else {
            return;
        };

        let account_id = AccountId::from(uuid::Uuid::nil());
        let currency = Currency::USD;
        let last = last.map(|d| (d.effective, d.values));
        let created_at = Utc::now();

        let mut updates: Vec<SnapshotOrEntry> = Vec::new();
        for op in &plan {
            match op.kind.as_str() {
                "entry" => {
                    if let Some(entry) = entries.get(op.idx) {
                        updates.push(SnapshotOrEntry::Entry {
                            effective: op.effective,
                            tx_index: op.tx_index,
                            created_at,
                            entry,
                        });
                    }
                }
                "snapshot" => {
                    if let Some(snap) = snapshots.get(op.idx) {
                        updates.push(SnapshotOrEntry::Snapshot {
                            effective: op.effective,
                            values: snap.values.clone(),
                        });
                    }
                }
                _ => {}
            }
        }

        let mut data = EffectiveBalanceData::new(account_id, currency, last, 0, updates);
        data.re_calculate_snapshots(chrono::Utc::now());
        let _ = data
            .into_snapshots(JournalId::from(uuid::Uuid::nil()))
            .count();
    }
}

#[cfg(feature = "fuzz")]
pub use __fuzz::fuzz_recalculate;
