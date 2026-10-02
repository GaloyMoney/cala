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

use data::{EffectiveBalanceData, SnapshotOrEntry};
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
        let mut out = Vec::new();
        for ((account_id, currency), data) in all_data {
            self.rewrite_pair_streaming(
                &mut *op, journal_id, account_id, currency, effective, data, created_at, &mut out,
            )
            .await?;
        }
        self.flush_rewrite_out(op, journal_id, &mut out).await?;

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
        let mut out = Vec::new();
        for ((account_id, currency), data) in all_data {
            let anchor = earliest[&(account_id, currency)];
            self.rewrite_pair_streaming(
                &mut *op,
                journal_id,
                account_id,
                currency,
                anchor,
                data,
                rewritten_at,
                &mut out,
            )
            .await?;
        }
        self.flush_rewrite_out(op, journal_id, &mut out).await?;

        Ok(())
    }

    /// Replay one pair's rewrite in bounded slices: delete the pair's
    /// futures after `anchor` piece by piece, fold each piece merged with
    /// the pair's batch entries, and flush the rebuilt history in chunks —
    /// all inside the caller's transaction. A backdated entry anchored
    /// millions of versions back can no longer grow one statement, one
    /// jsonb array, or the in-memory replay beyond a slice.
    async fn rewrite_pair_streaming(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        journal_id: JournalId,
        account_id: AccountId,
        currency: Currency,
        anchor: NaiveDate,
        mut data: EffectiveBalanceData<'_>,
        rewritten_at: DateTime<Utc>,
        out: &mut Vec<EffectiveBalanceSnapshot>,
    ) -> Result<(), BalanceError> {
        const REWRITE_SLICE: i64 = 50_000;
        const REWRITE_FLUSH: usize = 5_000;
        // Only rows strictly after the anchor are rewritten; `(anchor, i32::MAX)`
        // as the initial cursor excludes every row on the anchor date itself.
        let mut cursor = (anchor, i32::MAX);
        let mut entries = data.take_updates();
        entries.sort();
        let mut entries = entries.into_iter().peekable();
        loop {
            let mut slice = self
                .repo
                .delete_futures_slice(
                    &mut *op,
                    journal_id,
                    account_id,
                    currency.code(),
                    cursor,
                    REWRITE_SLICE,
                )
                .await?;
            // DELETE ... RETURNING does not guarantee row order; the replay
            // folds in (effective, version) order, so sort the bounded slice.
            slice.sort_by_key(|(effective, version, _)| (*effective, *version));
            let has_more = slice.len() as i64 == REWRITE_SLICE;
            for (effective, version, values) in slice {
                while let Some(entry) = entries.peek() {
                    // A pre-existing row sorts before any batch entry
                    // sharing its effective date (see SnapshotOrEntry::cmp).
                    if entry.effective() < &effective {
                        let entry = entries.next().expect("entry was peeked");
                        out.push(data.fold_next(journal_id, entry, rewritten_at));
                        if out.len() >= REWRITE_FLUSH {
                            self.flush_rewrite_out(&mut *op, journal_id, out).await?;
                        }
                    } else {
                        break;
                    }
                }
                let values =
                    serde_json::from_value(values).expect("Failed to deserialize balance snapshot");
                out.push(data.fold_next(
                    journal_id,
                    SnapshotOrEntry::Snapshot { effective, values },
                    rewritten_at,
                ));
                cursor = (effective, version);
                if out.len() >= REWRITE_FLUSH {
                    self.flush_rewrite_out(&mut *op, journal_id, out).await?;
                }
            }
            if !has_more {
                break;
            }
        }
        for entry in entries {
            out.push(data.fold_next(journal_id, entry, rewritten_at));
            if out.len() >= REWRITE_FLUSH {
                self.flush_rewrite_out(&mut *op, journal_id, out).await?;
            }
        }
        Ok(())
    }

    async fn flush_rewrite_out(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        journal_id: JournalId,
        out: &mut Vec<EffectiveBalanceSnapshot>,
    ) -> Result<(), BalanceError> {
        if out.is_empty() {
            return Ok(());
        }
        self.repo
            .insert_new_snapshots(op, journal_id, std::mem::take(out))
            .await
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

#[cfg(test)]
mod tests {
    //! Oracle tests for the streaming rewrite fold: the buffered replay
    //! (`re_calculate_snapshots` + `into_snapshots`) is the reference, and
    //! the sliced merge the driver performs must reproduce it exactly.
    use super::data::{EffectiveBalanceData, SnapshotOrEntry};
    use super::*;
    use cala_types::balance::{BalanceAmount, BalanceSnapshot};
    use cala_types::primitives::{DebitOrCredit, EntryId, Layer, TransactionId};
    use rust_decimal::Decimal;

    fn journal_id() -> JournalId {
        JournalId::from(uuid::Uuid::nil())
    }

    fn account_id() -> AccountId {
        AccountId::from(uuid::Uuid::from_u128(42))
    }

    fn mk_snapshot(settled_dr: i64, version: u32) -> BalanceSnapshot {
        let entry_id = EntryId::from(uuid::Uuid::from_u128(version as u128 + 1000));
        let time = Utc::now();
        BalanceSnapshot {
            journal_id: journal_id(),
            account_id: account_id(),
            entry_id,
            currency: Currency::USD,
            settled: BalanceAmount {
                dr_balance: Decimal::from(settled_dr),
                cr_balance: Decimal::ZERO,
                entry_id,
                modified_at: time,
            },
            pending: BalanceAmount {
                dr_balance: Decimal::ZERO,
                cr_balance: Decimal::ZERO,
                entry_id,
                modified_at: time,
            },
            encumbrance: BalanceAmount {
                dr_balance: Decimal::ZERO,
                cr_balance: Decimal::ZERO,
                entry_id,
                modified_at: time,
            },
            version,
            modified_at: time,
            created_at: time,
        }
    }

    fn mk_entry(id: u128, sequence: u32, units: i64) -> EntryValues {
        EntryValues {
            id: EntryId::from(uuid::Uuid::from_u128(id)),
            version: 1,
            transaction_id: TransactionId::from(uuid::Uuid::from_u128(7)),
            journal_id: journal_id(),
            account_id: account_id(),
            entry_type: "TEST".to_string(),
            sequence,
            layer: Layer::Settled,
            units: Decimal::from(units),
            currency: Currency::USD,
            direction: DebitOrCredit::Debit,
            description: None,
            metadata: None,
        }
    }

    fn d(day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2024, 1, day).unwrap()
    }

    fn e<'a>(effective: NaiveDate, tx_index: usize, entry: &'a EntryValues) -> SnapshotOrEntry<'a> {
        SnapshotOrEntry::Entry {
            effective,
            tx_index,
            created_at: Utc::now(),
            entry,
        }
    }

    fn s(effective: NaiveDate, snapshot: BalanceSnapshot) -> SnapshotOrEntry<'static> {
        SnapshotOrEntry::Snapshot {
            effective,
            values: snapshot,
        }
    }

    fn updates_fixture<'a>(
        entry_a: &'a EntryValues,
        entry_b: &'a EntryValues,
    ) -> Vec<SnapshotOrEntry<'a>> {
        vec![
            s(d(3), mk_snapshot(100, 1)),
            s(d(3), mk_snapshot(110, 2)),
            s(d(5), mk_snapshot(130, 1)),
            e(d(2), 0, entry_a),
            e(d(3), 1, entry_a),
            e(d(1), 2, entry_b),
        ]
    }

    fn rows_fixture() -> Vec<(NaiveDate, BalanceSnapshot)> {
        vec![
            (d(3), mk_snapshot(100, 1)),
            (d(3), mk_snapshot(110, 2)),
            (d(5), mk_snapshot(130, 1)),
        ]
    }

    fn buffered_reference<'a>(updates: Vec<SnapshotOrEntry<'a>>) -> Vec<EffectiveBalanceSnapshot> {
        let mut data = EffectiveBalanceData::new(
            account_id(),
            Currency::USD,
            Some((d(1), mk_snapshot(90, 7))),
            11,
            updates,
        );
        data.re_calculate_snapshots(Utc::now());
        data.into_snapshots(journal_id()).collect()
    }

    fn snapshot_tuple(snap: &EffectiveBalanceSnapshot) -> (NaiveDate, u32, u32, Decimal, Decimal) {
        (
            snap.effective,
            snap.version,
            snap.all_time_version,
            snap.settled.dr_balance,
            snap.settled.cr_balance,
        )
    }

    #[test]
    fn streamed_fold_matches_buffered_reference() {
        let entry_a = mk_entry(5001, 1, 10);
        let entry_b = mk_entry(5002, 1, 25);
        let expected: Vec<_> = buffered_reference(updates_fixture(&entry_a, &entry_b))
            .iter()
            .map(snapshot_tuple)
            .collect();

        let mut data = EffectiveBalanceData::new(
            account_id(),
            Currency::USD,
            Some((d(1), mk_snapshot(90, 7))),
            11,
            Vec::new(),
        );
        let mut updates = updates_fixture(&entry_a, &entry_b);
        updates.sort();
        let actual: Vec<_> = updates
            .into_iter()
            .map(|u| data.fold_next(journal_id(), u, Utc::now()))
            .map(|snap| snapshot_tuple(&snap))
            .collect();

        assert_eq!(actual, expected);
    }

    #[test]
    fn sliced_merge_reproduces_full_sort_order() {
        // Simulate rewrite_pair_streaming's feed: rows stream in
        // (effective, version) order in slices, and each row first emits the
        // pending entries whose effective date is strictly before it — the
        // same sequence a full sort produces.
        let entry_a = mk_entry(5001, 1, 10);
        let entry_b = mk_entry(5002, 1, 25);
        let expected: Vec<_> = buffered_reference(updates_fixture(&entry_a, &entry_b))
            .iter()
            .map(snapshot_tuple)
            .collect();

        let mut entries: Vec<SnapshotOrEntry> = vec![
            e(d(2), 0, &entry_a),
            e(d(3), 1, &entry_a),
            e(d(1), 2, &entry_b),
        ];
        entries.sort();
        let mut entries = entries.into_iter().peekable();

        let mut data = EffectiveBalanceData::new(
            account_id(),
            Currency::USD,
            Some((d(1), mk_snapshot(90, 7))),
            11,
            Vec::new(),
        );
        let mut actual = Vec::new();
        // Slice size 2 to cross a slice boundary mid-merge.
        for slice in rows_fixture().chunks(2) {
            for (effective, values) in slice {
                while let Some(entry) = entries.peek() {
                    if entry.effective() < effective {
                        let entry = entries.next().unwrap();
                        actual.push(snapshot_tuple(&data.fold_next(
                            journal_id(),
                            entry,
                            Utc::now(),
                        )));
                    } else {
                        break;
                    }
                }
                actual.push(snapshot_tuple(&data.fold_next(
                    journal_id(),
                    SnapshotOrEntry::Snapshot {
                        effective: *effective,
                        values: values.clone(),
                    },
                    Utc::now(),
                )));
            }
        }
        for entry in entries {
            actual.push(snapshot_tuple(&data.fold_next(
                journal_id(),
                entry,
                Utc::now(),
            )));
        }

        assert_eq!(actual, expected);
    }
}
