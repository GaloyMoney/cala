//! Effective balance series per (calculation, dimension tuple).
//!
//! Reads see what the calculation rollup has materialised. A caller that
//! needs completeness as of a moment fences first with
//! `CalaLedger::calculation_rollup_status().await_completion(timeout)` or
//! `CalaLedger::await_calculation_frontier(frontier, timeout)`; fencing the
//! EC rollup says nothing about calculations. Entries whose outbox sequence
//! is at or below a calculation's (or attachment's) `activation_sequence`
//! are not in its series.
mod repo;

use chrono::{DateTime, NaiveDate, Utc};
use es_entity::errlanes::{lanes, Fail};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use std::collections::HashMap;

use cala_types::{
    calculation::{CalculationBalanceId, Window},
    primitives::{AccountId, CalculationId, Currency, JournalId},
};

use crate::{
    balance::{
        AccountBalance, BalanceRange, CumulativeBalances, EffectiveBalanceSeries, EffectiveDates,
        EffectivePeriods, InvalidEffectiveDates, InvalidEffectivePeriods,
        MAX_EFFECTIVE_PERIODS_WORK,
    },
    effective::{cumulative::fold_cumulative, delta::DateDelta, periods::project_series},
    error::CalaFault,
};

use super::error::CalculationBalanceNotFound;
use repo::*;

/// Keyset cursor for
/// [`CalculationEffectiveBalances::list_cumulative_for_account`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CalculationSeriesCursor {
    pub currency: Currency,
    pub dimensions: serde_json::Value,
}

#[derive(Clone)]
pub struct CalculationEffectiveBalances {
    repo: CalculationEffectiveBalanceRepo,
}

impl CalculationEffectiveBalances {
    pub(crate) fn new(pool: &PgPool) -> Self {
        Self {
            repo: CalculationEffectiveBalanceRepo::new(pool),
        }
    }

    /// The series' cumulative balance at the end of `date`: its latest row
    /// with `effective <= date`. Reads what is materialised; fence on the
    /// calculation rollup for completeness (see the module docs).
    /// A unit in `BackfillStatus::InProgress` is incomplete; fence with
    /// `Calculations::await_backfill`.
    #[es_entity::errlanes::instrument(
        level = "debug",
        name = "cala_ledger.calculation.effective.find_cumulative",
        skip(self)
    )]
    pub async fn find_cumulative(
        &self,
        id: &CalculationBalanceId,
        date: NaiveDate,
    ) -> Result<AccountBalance, Fail<CalculationBalanceNotFound, lanes!(Transient, Fatal)>> {
        self.find_cumulative_in_op(self.repo.pool(), id, date).await
    }

    /// [`Self::find_cumulative`] within a caller-supplied executor.
    /// A unit in `BackfillStatus::InProgress` is incomplete; fence with
    /// `Calculations::await_backfill`.
    #[es_entity::errlanes::instrument(
        level = "debug",
        name = "cala_ledger.calculation.effective.find_cumulative_in_op",
        skip(self, op)
    )]
    pub async fn find_cumulative_in_op(
        &self,
        op: impl es_entity::IntoOneTimeExecutor<'_>,
        id: &CalculationBalanceId,
        date: NaiveDate,
    ) -> Result<AccountBalance, Fail<CalculationBalanceNotFound, lanes!(Transient, Fatal)>> {
        Ok(self
            .repo
            .find_in_op(op, id, date)
            .await?
            .ok_or_else(|| CalculationBalanceNotFound(id.clone()))?)
    }

    /// [`Self::find_cumulative`] for many series; absent series are
    /// omitted. Reads what is materialised; fence on the calculation rollup
    /// for completeness (see the module docs).
    /// A unit in `BackfillStatus::InProgress` is incomplete; fence with
    /// `Calculations::await_backfill`.
    #[es_entity::errlanes::instrument(level = "debug", name = "cala_ledger.calculation.effective.find_all_cumulative", skip(self, ids), fields(ids_count = ids.len()))]
    pub async fn find_all_cumulative(
        &self,
        ids: &[CalculationBalanceId],
        date: NaiveDate,
    ) -> Result<HashMap<CalculationBalanceId, AccountBalance>, CalaFault> {
        self.repo.find_all(ids, date).await
    }

    /// Opening balance (end of the day before `from`), closing balance (end
    /// of `until`, default today) and their difference. Reads what is
    /// materialised; fence on the calculation rollup for completeness (see
    /// the module docs).
    /// A unit in `BackfillStatus::InProgress` is incomplete; fence with
    /// `Calculations::await_backfill`.
    #[es_entity::errlanes::instrument(
        level = "debug",
        name = "cala_ledger.calculation.effective.find_in_range",
        skip(self)
    )]
    pub async fn find_in_range(
        &self,
        id: &CalculationBalanceId,
        from: NaiveDate,
        until: Option<NaiveDate>,
    ) -> Result<BalanceRange, Fail<CalculationBalanceNotFound, lanes!(Transient, Fatal)>> {
        let mut ranges = self
            .find_all_in_range(std::slice::from_ref(id), from, until)
            .await?;
        Ok(ranges
            .remove(id)
            .ok_or_else(|| CalculationBalanceNotFound(id.clone()))?)
    }

    /// [`Self::find_in_range`] for many series; series without a closing
    /// row are omitted. Reads what is materialised; fence on the
    /// calculation rollup for completeness (see the module docs).
    /// A unit in `BackfillStatus::InProgress` is incomplete; fence with
    /// `Calculations::await_backfill`.
    #[es_entity::errlanes::instrument(level = "debug", name = "cala_ledger.calculation.effective.find_all_in_range", skip(self, ids), fields(ids_count = ids.len()))]
    pub async fn find_all_in_range(
        &self,
        ids: &[CalculationBalanceId],
        from: NaiveDate,
        until: Option<NaiveDate>,
    ) -> Result<HashMap<CalculationBalanceId, BalanceRange>, CalaFault> {
        self.find_all_in_range_in_op(self.repo.pool(), ids, from, until)
            .await
    }

    /// [`Self::find_all_in_range`] within a caller-supplied executor.
    /// A unit in `BackfillStatus::InProgress` is incomplete; fence with
    /// `Calculations::await_backfill`.
    #[es_entity::errlanes::instrument(level = "debug", name = "cala_ledger.calculation.effective.find_all_in_range_in_op", skip(self, op, ids), fields(ids_count = ids.len()))]
    pub async fn find_all_in_range_in_op(
        &self,
        op: impl es_entity::IntoOneTimeExecutor<'_>,
        ids: &[CalculationBalanceId],
        from: NaiveDate,
        until: Option<NaiveDate>,
    ) -> Result<HashMap<CalculationBalanceId, BalanceRange>, CalaFault> {
        let ranges = self.repo.find_range_all(op, ids, from, until).await?;
        Ok(ranges
            .into_iter()
            .filter_map(|(id, (start, start_version, end, end_version))| {
                BalanceRange::from_bounds(start, start_version, end, end_version)
                    .map(|range| (id, range))
            })
            .collect())
    }

    /// Every `(currency, dimension tuple)` series of one account or set
    /// under one calculation, as of `date`, ordered by currency then
    /// dimensions. This is how a dimensioned caller discovers its tuples.
    /// Reads what is materialised; fence on the calculation rollup for
    /// completeness (see the module docs).
    /// A unit in `BackfillStatus::InProgress` is incomplete; fence with
    /// `Calculations::await_backfill`.
    #[es_entity::errlanes::instrument(
        level = "debug",
        name = "cala_ledger.calculation.effective.list_cumulative_for_account",
        skip(self)
    )]
    pub async fn list_cumulative_for_account(
        &self,
        journal_id: JournalId,
        calculation_id: CalculationId,
        account_id: impl Into<AccountId> + std::fmt::Debug,
        date: NaiveDate,
        args: es_entity::PaginatedQueryArgs<CalculationSeriesCursor>,
    ) -> Result<
        es_entity::PaginatedQueryRet<(Window, AccountBalance), CalculationSeriesCursor>,
        CalaFault,
    > {
        self.repo
            .list_for_account(journal_id, calculation_id, account_id.into(), date, args)
            .await
    }

    /// The series of whole `periods` for one calculation series. See
    /// [`Self::find_all_periods_in_op`]; a series with no row at or before
    /// the last period's end is [`CalculationBalanceNotFound`].
    /// A unit in `BackfillStatus::InProgress` is incomplete; fence with
    /// `Calculations::await_backfill`.
    #[es_entity::errlanes::instrument(
        level = "debug",
        name = "cala_ledger.calculation.effective.find_periods",
        skip(self)
    )]
    pub async fn find_periods(
        &self,
        id: &CalculationBalanceId,
        periods: &EffectivePeriods,
    ) -> Result<EffectiveBalanceSeries, Fail<CalculationBalanceNotFound, lanes!(Transient, Fatal)>>
    {
        Ok(self
            .find_series_in_op(self.repo.pool(), std::slice::from_ref(id), periods)
            .await?
            .remove(id)
            .ok_or_else(|| CalculationBalanceNotFound(id.clone()))?)
    }

    /// [`Self::find_all_periods_in_op`] on the ledger's pool.
    /// A unit in `BackfillStatus::InProgress` is incomplete; fence with
    /// `Calculations::await_backfill`.
    #[es_entity::errlanes::instrument(level = "debug", name = "cala_ledger.calculation.effective.find_all_periods", skip(self, ids), fields(ids_count = ids.len(), periods = periods.len()))]
    pub async fn find_all_periods(
        &self,
        ids: &[CalculationBalanceId],
        periods: &EffectivePeriods,
    ) -> Result<
        HashMap<CalculationBalanceId, EffectiveBalanceSeries>,
        Fail<InvalidEffectivePeriods, lanes!(Transient, Fatal)>,
    > {
        self.find_all_periods_in_op(self.repo.pool(), ids, periods)
            .await
    }

    /// The series of whole `periods` for every calculation series in `ids`,
    /// in one statement, with the semantics of
    /// `balances().effective().find_all_periods_in_op`:
    /// explicit cutoffs, absent ids omitted, zero periods before a series'
    /// first row, carried periods after its last, and the same work limit.
    /// Reads what is materialised; fence on the calculation rollup for
    /// completeness (see the module docs).
    /// A unit in `BackfillStatus::InProgress` is incomplete; fence with
    /// `Calculations::await_backfill`.
    #[es_entity::errlanes::instrument(level = "debug", name = "cala_ledger.calculation.effective.find_all_periods_in_op", skip(self, op, ids), fields(ids_count = ids.len(), periods = periods.len()))]
    pub async fn find_all_periods_in_op(
        &self,
        op: impl es_entity::IntoOneTimeExecutor<'_>,
        ids: &[CalculationBalanceId],
        periods: &EffectivePeriods,
    ) -> Result<
        HashMap<CalculationBalanceId, EffectiveBalanceSeries>,
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
        ids: &[CalculationBalanceId],
        periods: &EffectivePeriods,
    ) -> Result<HashMap<CalculationBalanceId, EffectiveBalanceSeries>, CalaFault> {
        Ok(self
            .find_cumulative_at_unchecked(op, ids, periods.dates())
            .await?
            .into_iter()
            .map(|(id, cumulative)| (id, project_series(periods, &cumulative)))
            .collect())
    }

    /// The cumulative balances of one calculation series at every date in
    /// `dates`. See [`Self::find_all_cumulative_at_in_op`]; a series with no
    /// row at or before the last date is [`CalculationBalanceNotFound`].
    /// Reads what is materialised; fence on the calculation rollup for
    /// completeness (see the module docs).
    /// A unit in `BackfillStatus::InProgress` is incomplete; fence with
    /// `Calculations::await_backfill`.
    #[es_entity::errlanes::instrument(
        level = "debug",
        name = "cala_ledger.calculation.effective.find_cumulative_at",
        skip(self)
    )]
    pub async fn find_cumulative_at(
        &self,
        id: &CalculationBalanceId,
        dates: &EffectiveDates,
    ) -> Result<CumulativeBalances, Fail<CalculationBalanceNotFound, lanes!(Transient, Fatal)>>
    {
        Ok(self
            .find_cumulative_at_unchecked(self.repo.pool(), std::slice::from_ref(id), dates)
            .await?
            .remove(id)
            .ok_or_else(|| CalculationBalanceNotFound(id.clone()))?)
    }

    /// [`Self::find_all_cumulative_at_in_op`] on the ledger's pool.
    /// A unit in `BackfillStatus::InProgress` is incomplete; fence with
    /// `Calculations::await_backfill`.
    #[es_entity::errlanes::instrument(level = "debug", name = "cala_ledger.calculation.effective.find_all_cumulative_at", skip(self, ids), fields(ids_count = ids.len(), dates = dates.len()))]
    pub async fn find_all_cumulative_at(
        &self,
        ids: &[CalculationBalanceId],
        dates: &EffectiveDates,
    ) -> Result<
        HashMap<CalculationBalanceId, CumulativeBalances>,
        Fail<InvalidEffectiveDates, lanes!(Transient, Fatal)>,
    > {
        self.find_all_cumulative_at_in_op(self.repo.pool(), ids, dates)
            .await
    }

    /// The cumulative balance of every calculation series in `ids` at every
    /// date in `dates` (Twisp's `effective.cumulative` on a calculation, for
    /// many series and dates at once), in one statement, with the semantics
    /// of `balances().effective().find_all_cumulative_at_in_op`: explicit
    /// dates (the database clock is never consulted), absent ids omitted,
    /// zero balances before a series' first row, the latest balance carried
    /// forward after its last, and the same work limit
    /// (`ids.len() * dates.len()` above [`MAX_EFFECTIVE_PERIODS_WORK`] is
    /// [`InvalidEffectiveDates`] before any statement runs). For a present
    /// id, `range(from, until)` equals
    /// [`Self::find_all_in_range_in_op`]`(&[id], from, Some(until))[id]`.
    /// Reads what is materialised; fence with
    /// `CalaLedger::calculation_rollup_status().await_completion(..)` or
    /// `CalaLedger::await_calculation_frontier(..)` for completeness (see the
    /// module docs).
    /// A unit in `BackfillStatus::InProgress` is incomplete; fence with
    /// `Calculations::await_backfill`.
    #[es_entity::errlanes::instrument(level = "debug", name = "cala_ledger.calculation.effective.find_all_cumulative_at_in_op", skip(self, op, ids), fields(ids_count = ids.len(), dates = dates.len()))]
    pub async fn find_all_cumulative_at_in_op(
        &self,
        op: impl es_entity::IntoOneTimeExecutor<'_>,
        ids: &[CalculationBalanceId],
        dates: &EffectiveDates,
    ) -> Result<
        HashMap<CalculationBalanceId, CumulativeBalances>,
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
        ids: &[CalculationBalanceId],
        dates: &EffectiveDates,
    ) -> Result<HashMap<CalculationBalanceId, CumulativeBalances>, CalaFault> {
        let checkpoints = self.repo.find_checkpoints(op, ids, dates.dates()).await?;
        Ok(checkpoints
            .into_iter()
            .filter_map(|(id, checkpoints)| {
                let base = (id.journal_id, id.account_id, id.currency);
                fold_cumulative(base, dates, &checkpoints).map(|cumulative| (id, cumulative))
            })
            .collect())
    }

    pub(crate) async fn apply_deltas_in_op(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        journal_id: JournalId,
        deltas: &[DateDelta<(CalculationId, Window)>],
        modified_at: DateTime<Utc>,
    ) -> Result<(), CalaFault> {
        self.repo
            .apply_deltas_in_op(op, journal_id, deltas, modified_at)
            .await
    }
}
