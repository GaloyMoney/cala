use crate::error::CalaFault;
use es_entity::errlanes::{lanes, Fail};
#[cfg(test)]
mod differential_tests;
#[cfg(any(test, feature = "fuzz"))]
mod fold_oracle;
mod repo;

use chrono::{DateTime, NaiveDate, Utc};
use sqlx::PgPool;
use std::collections::{HashMap, HashSet};

use cala_types::{balance::EffectiveBalanceSnapshot, entry::EntryValues, primitives::*};

use crate::primitives::JournalId;

use super::{
    account_balance::*,
    cursor::{
        AccountBalanceByCurrencyCursor, AccountBalanceCursor, EffectiveBalancesModifiedCursor,
    },
    error::BalanceNotFound,
    EcRollupTxn,
};

use crate::effective::{
    cumulative::{fold_cumulative, CumulativeBalances, EffectiveDates, InvalidEffectiveDates},
    delta::DeltaAccumulator,
    periods::{
        project_series, EffectiveBalanceSeries, EffectivePeriods, InvalidEffectivePeriods,
        MAX_EFFECTIVE_PERIODS_WORK,
    },
};
use repo::*;

#[derive(Clone)]
pub struct EffectiveBalances {
    repo: EffectiveBalanceRepo,
    pool: PgPool,
}
impl EffectiveBalances {
    pub(crate) fn new(pool: &PgPool) -> Self {
        Self {
            repo: EffectiveBalanceRepo::new(pool),
            pool: pool.clone(),
        }
    }

    #[es_entity::errlanes::instrument(
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
    ) -> Result<AccountBalance, Fail<BalanceNotFound, lanes!(Transient, Fatal)>> {
        self.repo
            .find(journal_id, account_id.into(), currency, date)
            .await
    }

    #[es_entity::errlanes::instrument(
        level = "debug",
        name = "cala_ledger.balance.effective.find_cumulative_in_op",
        skip(self, op)
    )]
    pub async fn find_cumulative_in_op(
        &self,
        op: impl es_entity::IntoOneTimeExecutor<'_>,
        journal_id: JournalId,
        account_id: impl Into<AccountId> + std::fmt::Debug,
        currency: Currency,
        date: NaiveDate,
    ) -> Result<AccountBalance, Fail<BalanceNotFound, lanes!(Transient, Fatal)>> {
        self.repo
            .find_in_op(op, journal_id, account_id.into(), currency, date)
            .await
    }

    #[es_entity::errlanes::instrument(
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
    ) -> Result<BalanceRange, Fail<BalanceNotFound, lanes!(Transient, Fatal)>> {
        match self
            .repo
            .find_range(journal_id, account_id, currency, from, until)
            .await?
        {
            (start, Some(end), version_diff) => Ok(BalanceRange::new(start, end, version_diff)),
            _ => Err(BalanceNotFound(journal_id, account_id, currency).into()),
        }
    }

    #[es_entity::errlanes::instrument(level = "debug", name = "cala_ledger.balance.effective.find_all_cumulative", skip(self, ids), fields(ids_count = ids.len()))]
    pub async fn find_all_cumulative(
        &self,
        ids: &[BalanceId],
        date: NaiveDate,
    ) -> Result<HashMap<BalanceId, AccountBalance>, CalaFault> {
        self.repo.find_all(ids, date).await
    }

    #[es_entity::errlanes::instrument(
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
        CalaFault,
    > {
        self.repo
            .list_for_account(journal_id, account_id.into(), date, args)
            .await
    }

    #[es_entity::errlanes::instrument(
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
    ) -> Result<es_entity::PaginatedQueryRet<AccountBalance, AccountBalanceCursor>, CalaFault> {
        self.repo
            .list_for_accounts(journal_id, account_ids, date, args)
            .await
    }

    #[es_entity::errlanes::instrument(level = "debug", name = "cala_ledger.balance.effective.find_all_in_range", skip(self, ids), fields(ids_count = ids.len()))]
    pub async fn find_all_in_range(
        &self,
        ids: &[BalanceId],
        from: NaiveDate,
        until: Option<NaiveDate>,
    ) -> Result<HashMap<BalanceId, BalanceRange>, CalaFault> {
        let ranges = self.repo.find_range_all(ids, from, until).await?;
        Ok(ranges
            .into_iter()
            .filter_map(|(id, (start, start_version, end, end_version))| {
                BalanceRange::from_bounds(start, start_version, end, end_version)
                    .map(|range| (id, range))
            })
            .collect())
    }

    /// The series of whole `periods` for one balance: per period the opening
    /// cumulative balance, the activity inside it and the closing cumulative
    /// balance, served by one statement.
    ///
    /// See [`Self::find_all_periods_in_op`] for the semantics shared by the
    /// period reads. A balance with no row at or before the last period's end
    /// (including an unknown account) is [`BalanceNotFound`].
    #[es_entity::errlanes::instrument(
        level = "debug",
        name = "cala_ledger.balance.effective.find_periods",
        skip(self)
    )]
    pub async fn find_periods(
        &self,
        journal_id: JournalId,
        account_id: impl Into<AccountId> + std::fmt::Debug,
        currency: Currency,
        periods: &EffectivePeriods,
    ) -> Result<EffectiveBalanceSeries, Fail<BalanceNotFound, lanes!(Transient, Fatal)>> {
        let account_id = account_id.into();
        let id = (journal_id, account_id, currency);
        self.find_series_in_op(&self.pool, &[id], periods)
            .await?
            .remove(&id)
            .ok_or_else(|| BalanceNotFound(journal_id, account_id, currency).into())
    }

    /// [`Self::find_all_periods_in_op`] on the ledger's pool.
    #[es_entity::errlanes::instrument(level = "debug", name = "cala_ledger.balance.effective.find_all_periods", skip(self, ids), fields(ids_count = ids.len(), periods = periods.len()))]
    pub async fn find_all_periods(
        &self,
        ids: &[BalanceId],
        periods: &EffectivePeriods,
    ) -> Result<
        HashMap<BalanceId, EffectiveBalanceSeries>,
        Fail<InvalidEffectivePeriods, lanes!(Transient, Fatal)>,
    > {
        self.find_all_periods_in_op(&self.pool, ids, periods).await
    }

    /// The series of whole `periods` for every id in `ids`, served by one
    /// statement however many ids or periods are requested. Each series has
    /// exactly `periods.len()` entries; per period `range.open` is the
    /// cumulative balance at the end of the previous period, `range.period`
    /// the activity inside it (`details.version` counts its entries) and
    /// `range.close` the cumulative balance at its end.
    ///
    /// * **Explicit dates.** Every cutoff comes from `periods`; the database
    ///   clock is never consulted. A caller wanting "up to today" builds the
    ///   periods from today's date.
    /// * **Absent ids.** An id whose account does not exist, or whose pair
    ///   has no row at or before the last period's end, is absent from the
    ///   map (as in [`Self::find_all_in_range`]). A currency the account
    ///   never used is absent too.
    /// * **Zero and carried periods.** Periods before the pair's first row
    ///   are all-zero; periods after its last row carry the last cumulative
    ///   balance forward with zero activity and `version == 0`.
    /// * **Work limit.** `ids.len() * (periods.len() + 1)` above
    ///   [`MAX_EFFECTIVE_PERIODS_WORK`] is rejected with
    ///   [`InvalidEffectivePeriods`] before any statement runs.
    /// * **Consistency.** The read sees what is materialised. Synchronous
    ///   accounts and sets are current as of commit; eventually-consistent
    ///   accounts and sets lag the streaming rollup, so a caller wanting a
    ///   complete picture fences first with
    ///   [`CalaLedger::ec_rollup_status`](crate::CalaLedger::ec_rollup_status)
    ///   `.await_completion(..)` (see [`Self::list_modified_since`]). Running
    ///   on a repeatable-read transaction lets several reads share one
    ///   snapshot.
    #[es_entity::errlanes::instrument(level = "debug", name = "cala_ledger.balance.effective.find_all_periods_in_op", skip(self, op, ids), fields(ids_count = ids.len(), periods = periods.len()))]
    pub async fn find_all_periods_in_op(
        &self,
        op: impl es_entity::IntoOneTimeExecutor<'_>,
        ids: &[BalanceId],
        periods: &EffectivePeriods,
    ) -> Result<
        HashMap<BalanceId, EffectiveBalanceSeries>,
        Fail<InvalidEffectivePeriods, lanes!(Transient, Fatal)>,
    > {
        let probes = ids.len().saturating_mul(periods.len() + 1);
        if probes > MAX_EFFECTIVE_PERIODS_WORK {
            return Err(InvalidEffectivePeriods.into());
        }
        Ok(self.find_series_in_op(op, ids, periods).await?)
    }

    async fn find_series_in_op(
        &self,
        op: impl es_entity::IntoOneTimeExecutor<'_>,
        ids: &[BalanceId],
        periods: &EffectivePeriods,
    ) -> Result<HashMap<BalanceId, EffectiveBalanceSeries>, CalaFault> {
        Ok(self
            .find_cumulative_at_unchecked(op, ids, periods.dates())
            .await?
            .into_iter()
            .map(|(id, cumulative)| (id, project_series(periods, &cumulative)))
            .collect())
    }

    /// The cumulative balances of one balance at every date in `dates`. See
    /// [`Self::find_all_cumulative_at_in_op`] for the semantics. A balance
    /// with no row at or before the last date (including an unknown account)
    /// is [`BalanceNotFound`].
    #[es_entity::errlanes::instrument(
        level = "debug",
        name = "cala_ledger.balance.effective.find_cumulative_at",
        skip(self)
    )]
    pub async fn find_cumulative_at(
        &self,
        journal_id: JournalId,
        account_id: impl Into<AccountId> + std::fmt::Debug,
        currency: Currency,
        dates: &EffectiveDates,
    ) -> Result<CumulativeBalances, Fail<BalanceNotFound, lanes!(Transient, Fatal)>> {
        let account_id = account_id.into();
        let id = (journal_id, account_id, currency);
        self.find_cumulative_at_unchecked(&self.pool, &[id], dates)
            .await?
            .remove(&id)
            .ok_or_else(|| BalanceNotFound(journal_id, account_id, currency).into())
    }

    /// [`Self::find_all_cumulative_at_in_op`] on the ledger's pool.
    #[es_entity::errlanes::instrument(level = "debug", name = "cala_ledger.balance.effective.find_all_cumulative_at", skip(self, ids), fields(ids_count = ids.len(), dates = dates.len()))]
    pub async fn find_all_cumulative_at(
        &self,
        ids: &[BalanceId],
        dates: &EffectiveDates,
    ) -> Result<
        HashMap<BalanceId, CumulativeBalances>,
        Fail<InvalidEffectiveDates, lanes!(Transient, Fatal)>,
    > {
        self.find_all_cumulative_at_in_op(&self.pool, ids, dates)
            .await
    }

    /// The cumulative balance of every id in `ids` at every date in `dates`
    /// (Twisp's `effective.cumulative`, for many balances and dates at once),
    /// served by one statement. [`CumulativeBalances::at`] is the balance as
    /// of one date and [`CumulativeBalances::range`] the range between two.
    ///
    /// * **Explicit dates.** Every date comes from `dates`; the database
    ///   clock is never consulted. A caller wanting "up to today" passes
    ///   today.
    /// * **Absent ids.** An id whose account does not exist, or whose pair
    ///   has no row at or before the last date, is absent from the map (as
    ///   in [`Self::find_all_in_range`]). A currency the account never used
    ///   is absent too.
    /// * **Zero fill and carry forward.** A date before the pair's first row
    ///   holds a zero balance; a date after its last row carries the latest
    ///   cumulative balance forward.
    /// * **Ranges.** For a present id, `range(from, until)` with `from - 1`
    ///   and `until` among `dates` equals
    ///   `find_all_in_range(&[id], from, Some(until))[id]`, including the
    ///   zero `open` when the pair had no row before `from`.
    /// * **Work limit.** `ids.len() * dates.len()` above
    ///   [`MAX_EFFECTIVE_PERIODS_WORK`] is rejected with
    ///   [`InvalidEffectiveDates`] before any statement runs.
    /// * **Consistency.** The read sees what is materialised. Synchronous
    ///   accounts and sets are current as of commit; eventually-consistent
    ///   accounts and sets lag the streaming rollup, so a caller wanting a
    ///   complete picture fences first with
    ///   [`CalaLedger::ec_rollup_status`](crate::CalaLedger::ec_rollup_status)
    ///   `.await_completion(..)`. Running on a repeatable-read transaction
    ///   lets several reads share one snapshot.
    #[es_entity::errlanes::instrument(level = "debug", name = "cala_ledger.balance.effective.find_all_cumulative_at_in_op", skip(self, op, ids), fields(ids_count = ids.len(), dates = dates.len()))]
    pub async fn find_all_cumulative_at_in_op(
        &self,
        op: impl es_entity::IntoOneTimeExecutor<'_>,
        ids: &[BalanceId],
        dates: &EffectiveDates,
    ) -> Result<
        HashMap<BalanceId, CumulativeBalances>,
        Fail<InvalidEffectiveDates, lanes!(Transient, Fatal)>,
    > {
        if ids.len().saturating_mul(dates.len()) > MAX_EFFECTIVE_PERIODS_WORK {
            return Err(InvalidEffectiveDates.into());
        }
        Ok(self.find_cumulative_at_unchecked(op, ids, dates).await?)
    }

    async fn find_cumulative_at_unchecked(
        &self,
        op: impl es_entity::IntoOneTimeExecutor<'_>,
        ids: &[BalanceId],
        dates: &EffectiveDates,
    ) -> Result<HashMap<BalanceId, CumulativeBalances>, CalaFault> {
        let checkpoints = self.repo.find_checkpoints(op, ids, dates.dates()).await?;
        Ok(ids
            .iter()
            .filter_map(|id| fold_cumulative(*id, dates, &checkpoints).map(|c| (*id, c)))
            .collect())
    }

    #[es_entity::errlanes::instrument(
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
    ) -> Result<es_entity::PaginatedQueryRet<BalanceRange, AccountBalanceByCurrencyCursor>, CalaFault>
    {
        self.repo
            .list_range_for_account(journal_id, account_id.into(), from, until, args)
            .await
    }

    #[es_entity::errlanes::instrument(
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
    ) -> Result<es_entity::PaginatedQueryRet<BalanceRange, AccountBalanceCursor>, CalaFault> {
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
    #[es_entity::errlanes::instrument(
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
        CalaFault,
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
        eligible: HashSet<(AccountId, Currency)>,
    ) -> Result<(), CalaFault> {
        let mut deltas = DeltaAccumulator::new();
        // Entries arrive grouped by transaction in landing order; the first
        // appearance of a transaction id fixes its position in the batch.
        let mut tx_indices: HashMap<TransactionId, usize> = HashMap::new();
        let empty = Vec::new();
        for entry in entries.iter() {
            let next_index = tx_indices.len();
            let tx_index = *tx_indices.entry(entry.transaction_id).or_insert(next_index);
            for account_id in mappings
                .get(&entry.account_id)
                .unwrap_or(&empty)
                .iter()
                .map(AccountId::from)
                .chain(std::iter::once(entry.account_id))
            {
                if eligible.contains(&(account_id, entry.currency)) {
                    deltas.push(account_id, effective, tx_index, entry);
                }
            }
        }

        self.repo
            .apply_deltas_in_op(op, journal_id, &deltas.into_deltas(), created_at)
            .await
    }

    /// EC counterpart of [`Self::update_cumulative_balances_in_op`] used by
    /// the streaming rollup: fans every transaction's entries in the batch
    /// into their EC ancestor sets and, for an entry whose leaf is an EC
    /// plain account (listed in `ec_leaves`), into that leaf's own
    /// cumulative-effective balance too.
    ///
    /// Batched per pair rather than per transaction: every entry of the
    /// batch is folded in memory into one delta per `(account, currency,
    /// effective date)`, and those deltas are applied by one set-based
    /// statement pair (see `EffectiveBalanceRepo::apply_deltas_in_op`). No
    /// stored balance row is loaded. A backdated transaction adds its delta
    /// to every later date row of the pairs it touches — one row update per
    /// later calendar date, once per batch rather than once per transaction —
    /// which is equivalent to applying the batch's transactions one at a
    /// time in landing order.
    #[es_entity::errlanes::instrument(
        level = "debug",
        name = "cala_ledger.balance.effective.apply_ec_rollup_batch_in_op",
        skip_all,
        fields(txns_count = txns.len())
    )]
    pub(crate) async fn apply_ec_rollup_batch_in_op(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        journal_id: JournalId,
        txns: &[EcRollupTxn<'_>],
        ec_mappings: &HashMap<AccountId, Vec<AccountSetId>>,
        ec_leaves: &HashSet<AccountId>,
    ) -> Result<(), CalaFault> {
        let targets = |account_id: &AccountId| {
            ec_mappings
                .get(account_id)
                .into_iter()
                .flatten()
                .map(AccountId::from)
                .chain(ec_leaves.get(account_id).copied())
        };

        let mut deltas = DeltaAccumulator::new();
        for (tx_index, tx) in txns.iter().enumerate() {
            for entry in tx.entries.iter().copied() {
                for target in targets(&entry.account_id) {
                    deltas.push(target, tx.effective, tx_index, entry);
                }
            }
        }
        if deltas.is_empty() {
            return Ok(());
        }

        // Every row the batch writes is stamped with the batch's newest
        // transaction time.
        let modified_at = txns
            .iter()
            .map(|tx| tx.created_at)
            .max()
            .expect("txns is non-empty: deltas was populated from it above");
        self.repo
            .apply_deltas_in_op(op, journal_id, &deltas.into_deltas(), modified_at)
            .await
    }
}

#[cfg(feature = "fuzz")]
mod __fuzz {
    //! Harness for the out-of-tree `effective_balance` fuzz target. Lives in
    //! this module so it can reach the `pub(super)` `EffectiveBalanceData`.
    use super::fold_oracle::{EffectiveBalanceData, SnapshotOrEntry};
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
