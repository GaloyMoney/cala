use chrono::{Datelike, Months, NaiveDate};
use es_entity::errlanes;

use crate::balance::account_balance::{AccountBalance, BalanceRange};

#[cfg(test)]
use super::cumulative::{fold_cumulative, Checkpoints};
use super::cumulative::{CumulativeBalances, EffectiveDates};
#[cfg(test)]
use cala_types::primitives::BalanceId;

/// Bounds on one request. Day granularity over a year is 366 periods.
pub const MAX_EFFECTIVE_PERIODS: usize = 400;
/// Bounds indexed checkpoint probes: ids × (periods + 1).
pub const MAX_EFFECTIVE_PERIODS_WORK: usize = 1_000_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PeriodGranularity {
    Day,
    Month,
    Year,
}

impl PeriodGranularity {
    fn is_start(self, date: NaiveDate) -> bool {
        match self {
            Self::Day => true,
            Self::Month => date.day() == 1,
            Self::Year => date.ordinal() == 1,
        }
    }

    fn next_start(self, start: NaiveDate) -> Option<NaiveDate> {
        match self {
            Self::Day => start.succ_opt(),
            Self::Month => start.checked_add_months(Months::new(1)),
            Self::Year => start.checked_add_months(Months::new(12)),
        }
    }
}

#[derive(Debug, errlanes::Rejection)]
#[rejection(code = "CALA_INVALID_EFFECTIVE_PERIODS")]
#[error(
    "effective periods must be aligned to their granularity, non-empty, and within the work limit"
)]
pub struct InvalidEffectivePeriods;

/// A half-open run of whole periods `[gte, lt)`, Twisp's `effective.periods` shape.
///
/// `gte` and `lt` must each be the first day of a period at `granularity`, and
/// `gte < lt`. Dates are always explicit: nothing built from this type consults
/// the database clock.
#[derive(Debug, Clone)]
pub struct EffectivePeriods {
    granularity: PeriodGranularity,
    /// First day of each period, ascending.
    starts: Vec<NaiveDate>,
    /// `starts[0] - 1 day`, then the last day of each period.
    /// Length = `starts.len() + 1`.
    dates: EffectiveDates,
}

impl EffectivePeriods {
    pub fn try_new(
        granularity: PeriodGranularity,
        gte: NaiveDate,
        lt: NaiveDate,
    ) -> Result<Self, InvalidEffectivePeriods> {
        if !granularity.is_start(gte) || !granularity.is_start(lt) || lt <= gte {
            return Err(InvalidEffectivePeriods);
        }
        let before = gte.pred_opt().ok_or(InvalidEffectivePeriods)?;
        let mut starts = Vec::new();
        let mut cutoffs = vec![before];
        let mut start = gte;
        while start < lt {
            if starts.len() == MAX_EFFECTIVE_PERIODS {
                return Err(InvalidEffectivePeriods);
            }
            let next = granularity
                .next_start(start)
                .ok_or(InvalidEffectivePeriods)?;
            starts.push(start);
            cutoffs.push(next.pred_opt().ok_or(InvalidEffectivePeriods)?);
            start = next;
        }
        Ok(Self {
            granularity,
            starts,
            dates: EffectiveDates::try_new(cutoffs).map_err(|_| InvalidEffectivePeriods)?,
        })
    }

    /// `count` consecutive periods starting at `gte`.
    pub fn try_from_count(
        granularity: PeriodGranularity,
        gte: NaiveDate,
        count: usize,
    ) -> Result<Self, InvalidEffectivePeriods> {
        if count == 0 || count > MAX_EFFECTIVE_PERIODS {
            return Err(InvalidEffectivePeriods);
        }
        let mut lt = gte;
        for _ in 0..count {
            lt = granularity.next_start(lt).ok_or(InvalidEffectivePeriods)?;
        }
        Self::try_new(granularity, gte, lt)
    }

    pub fn granularity(&self) -> PeriodGranularity {
        self.granularity
    }

    /// Number of periods.
    pub fn len(&self) -> usize {
        self.starts.len()
    }

    pub fn is_empty(&self) -> bool {
        self.starts.is_empty()
    }

    /// `(start, end)`, both inclusive, of each period in ascending order.
    pub fn bounds(&self) -> impl Iterator<Item = (NaiveDate, NaiveDate)> + '_ {
        self.starts
            .iter()
            .copied()
            .zip(self.dates.dates().iter().skip(1).copied())
    }

    /// `gte - 1 day`, then the last day of each period.
    pub(crate) fn dates(&self) -> &EffectiveDates {
        &self.dates
    }
}

/// One period of a series. `range.open` is the cumulative balance at the end of
/// the previous period, `range.period` the activity inside this period (its
/// `details.version` is the number of entries folded in the period, as in
/// `find_in_range`), `range.close` the cumulative balance at `end`.
#[derive(Debug, Clone)]
pub struct PeriodBalance {
    pub start: NaiveDate,
    pub end: NaiveDate,
    pub range: BalanceRange,
}

/// The series for one balance id: exactly `EffectivePeriods::len()` entries, ascending.
#[derive(Debug, Clone)]
pub struct EffectiveBalanceSeries {
    pub periods: Vec<PeriodBalance>,
}

impl EffectiveBalanceSeries {
    /// Twisp `accumulate: false` top-level balance: activity over the whole run,
    /// i.e. `last.close - first.open`. Also what callers use for an "overall" column.
    pub fn total_activity(&self) -> Option<AccountBalance> {
        let first = self.periods.first()?;
        let last = self.periods.last()?;
        let mut total = last.range.close.clone().derive_diff(&first.range.open);
        total.details.version = self
            .periods
            .iter()
            .map(|period| period.range.period.details.version)
            .sum();
        Some(total)
    }

    /// Cumulative balance at the start of the first period.
    pub fn opening(&self) -> Option<&AccountBalance> {
        self.periods.first().map(|period| &period.range.open)
    }

    /// Cumulative balance at the end of the last period.
    pub fn closing(&self) -> Option<&AccountBalance> {
        self.periods.last().map(|period| &period.range.close)
    }
}

/// Projects cumulative balances read at `periods.dates()` onto the periods:
/// each period is `cumulative.range(start, end)`.
pub(crate) fn project_series(
    periods: &EffectivePeriods,
    cumulative: &CumulativeBalances,
) -> EffectiveBalanceSeries {
    EffectiveBalanceSeries {
        periods: periods
            .bounds()
            .map(|(start, end)| PeriodBalance {
                start,
                end,
                range: cumulative
                    .range(start, end)
                    .expect("period bounds come from the periods' dates"),
            })
            .collect(),
    }
}

/// Folds the checkpoints fetched at `periods.dates()` into one series.
/// Returns `None` when the pair has no checkpoint at all, i.e. no row at or
/// before the last cutoff.
#[cfg(test)]
pub(crate) fn fold_series(
    id: BalanceId,
    periods: &EffectivePeriods,
    checkpoints: &Checkpoints,
) -> Option<EffectiveBalanceSeries> {
    fold_cumulative(id, periods.dates(), checkpoints)
        .map(|cumulative| project_series(periods, &cumulative))
}

#[cfg(test)]
mod tests {
    use super::super::cumulative::Checkpoint;
    use super::*;
    use crate::balance::snapshot::UNASSIGNED_ENTRY_ID;
    use cala_types::{
        balance::{BalanceAmount, BalanceSnapshot},
        primitives::{AccountId, Currency, DebitOrCredit, EntryId, JournalId},
    };
    use chrono::Utc;
    use rust_decimal::Decimal;
    use std::collections::HashMap;

    fn date(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    #[test]
    fn month_cutoffs_handle_leap_year() {
        let periods =
            EffectivePeriods::try_new(PeriodGranularity::Month, date(2024, 1, 1), date(2024, 4, 1))
                .unwrap();
        assert_eq!(periods.granularity(), PeriodGranularity::Month);
        assert_eq!(periods.len(), 3);
        assert!(!periods.is_empty());
        assert_eq!(
            periods.bounds().map(|(start, _)| start).collect::<Vec<_>>(),
            vec![date(2024, 1, 1), date(2024, 2, 1), date(2024, 3, 1)],
        );
        assert_eq!(
            periods.dates().dates(),
            &[
                date(2023, 12, 31),
                date(2024, 1, 31),
                date(2024, 2, 29),
                date(2024, 3, 31)
            ],
        );
    }

    #[test]
    fn year_and_day_cutoffs() {
        let year =
            EffectivePeriods::try_new(PeriodGranularity::Year, date(2024, 1, 1), date(2025, 1, 1))
                .unwrap();
        assert_eq!(
            year.dates().dates(),
            &[date(2023, 12, 31), date(2024, 12, 31)]
        );

        let days =
            EffectivePeriods::try_from_count(PeriodGranularity::Day, date(2024, 2, 28), 3).unwrap();
        assert_eq!(
            days.dates().dates(),
            &[
                date(2024, 2, 27),
                date(2024, 2, 28),
                date(2024, 2, 29),
                date(2024, 3, 1)
            ],
        );
        assert_eq!(
            days.bounds().collect::<Vec<_>>(),
            vec![
                (date(2024, 2, 28), date(2024, 2, 28)),
                (date(2024, 2, 29), date(2024, 2, 29)),
                (date(2024, 3, 1), date(2024, 3, 1)),
            ],
        );
    }

    #[test]
    fn rejects_unaligned_empty_and_too_many() {
        use PeriodGranularity::*;
        let jan_1 = date(2024, 1, 1);
        assert!(EffectivePeriods::try_new(Month, date(2024, 1, 15), date(2024, 4, 1)).is_err());
        assert!(EffectivePeriods::try_new(Month, jan_1, date(2024, 4, 2)).is_err());
        assert!(EffectivePeriods::try_new(Year, date(2024, 2, 1), date(2025, 1, 1)).is_err());
        assert!(EffectivePeriods::try_new(Month, jan_1, jan_1).is_err());
        assert!(EffectivePeriods::try_new(Month, date(2024, 4, 1), jan_1).is_err());
        assert!(EffectivePeriods::try_new(Day, jan_1, jan_1 + chrono::Days::new(401)).is_err());
        assert!(EffectivePeriods::try_new(Day, jan_1, jan_1 + chrono::Days::new(400)).is_ok());
        assert!(EffectivePeriods::try_new(Day, NaiveDate::MIN, jan_1).is_err());
        assert!(EffectivePeriods::try_from_count(Month, jan_1, 0).is_err());
        assert!(EffectivePeriods::try_from_count(Day, jan_1, 401).is_err());
    }

    fn balance_id() -> BalanceId {
        (
            JournalId::new(),
            AccountId::new(),
            "USD".parse::<Currency>().unwrap(),
        )
    }

    fn checkpoint(id: BalanceId, cumulative: i64, version: u32) -> Checkpoint {
        let entry_id = EntryId::new();
        let amount = |cr: i64| BalanceAmount {
            dr_balance: Decimal::ZERO,
            cr_balance: Decimal::from(cr),
            entry_id,
            modified_at: Utc::now(),
        };
        (
            AccountBalance::new(
                DebitOrCredit::Credit,
                BalanceSnapshot {
                    journal_id: id.0,
                    account_id: id.1,
                    currency: id.2,
                    version,
                    created_at: Utc::now(),
                    modified_at: Utc::now(),
                    entry_id,
                    settled: amount(cumulative),
                    pending: amount(0),
                    encumbrance: amount(0),
                },
            ),
            version,
        )
    }

    fn q1_2024() -> EffectivePeriods {
        EffectivePeriods::try_new(PeriodGranularity::Month, date(2024, 1, 1), date(2024, 4, 1))
            .unwrap()
    }

    #[test]
    fn fold_series_matches_hand_computed_example() {
        let id = balance_id();
        let checkpoints: Checkpoints = HashMap::from([
            ((date(2024, 1, 31), id), checkpoint(id, 120, 2)),
            ((date(2024, 2, 29), id), checkpoint(id, 170, 3)),
            ((date(2024, 3, 31), id), checkpoint(id, 200, 4)),
        ]);
        let series = fold_series(id, &q1_2024(), &checkpoints).expect("present");
        assert_eq!(series.periods.len(), 3);
        let rows: Vec<_> = series
            .periods
            .iter()
            .map(|p| {
                (
                    p.start,
                    p.end,
                    p.range.open.settled(),
                    p.range.period.settled(),
                    p.range.period.details.version,
                    p.range.close.settled(),
                )
            })
            .collect();
        let d = Decimal::from;
        assert_eq!(
            rows,
            vec![
                (date(2024, 1, 1), date(2024, 1, 31), d(0), d(120), 2, d(120)),
                (
                    date(2024, 2, 1),
                    date(2024, 2, 29),
                    d(120),
                    d(50),
                    1,
                    d(170)
                ),
                (
                    date(2024, 3, 1),
                    date(2024, 3, 31),
                    d(170),
                    d(30),
                    1,
                    d(200)
                ),
            ],
        );
        assert_eq!(series.opening().unwrap().settled(), d(0));
        assert_eq!(series.closing().unwrap().settled(), d(200));
        let total = series.total_activity().unwrap();
        assert_eq!(total.settled(), d(200));
        assert_eq!(total.details.version, 4);
    }

    #[test]
    fn fold_series_zero_leading_and_carried_trailing_periods() {
        let id = balance_id();
        let periods =
            EffectivePeriods::try_new(PeriodGranularity::Month, date(2024, 1, 1), date(2024, 5, 1))
                .unwrap();
        let checkpoints: Checkpoints =
            HashMap::from([((date(2024, 2, 29), id), checkpoint(id, 70, 3))]);
        let series = fold_series(id, &periods, &checkpoints).expect("present");
        assert_eq!(series.periods.len(), 4);

        let first = &series.periods[0].range;
        for balance in [&first.open, &first.period, &first.close] {
            assert_eq!(balance.settled(), Decimal::ZERO);
            assert_eq!(balance.details.version, 0);
            assert_eq!(balance.details.entry_id, EntryId::from(UNASSIGNED_ENTRY_ID));
            assert_eq!(balance.details.modified_at.timestamp(), 0);
            assert_eq!(balance.balance_type, DebitOrCredit::Credit);
        }

        let second = &series.periods[1].range;
        assert_eq!(second.open.settled(), Decimal::ZERO);
        assert_eq!(second.period.settled(), Decimal::from(70));
        assert_eq!(second.period.details.version, 3);
        assert_eq!(second.close.settled(), Decimal::from(70));

        for trailing in &series.periods[2..] {
            assert_eq!(trailing.range.open.settled(), Decimal::from(70));
            assert_eq!(trailing.range.period.settled(), Decimal::ZERO);
            assert_eq!(trailing.range.period.details.version, 0);
            assert_eq!(trailing.range.close.settled(), Decimal::from(70));
        }
    }

    #[test]
    fn fold_series_without_checkpoints_is_absent() {
        let id = balance_id();
        assert!(fold_series(id, &q1_2024(), &HashMap::new()).is_none());
    }
}
