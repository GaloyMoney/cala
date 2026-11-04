use chrono::NaiveDate;
use es_entity::errlanes;
use std::collections::HashMap;

use cala_types::primitives::BalanceId;

use crate::balance::account_balance::{AccountBalance, BalanceRange};

/// Bounds the dates in one request. Equals `MAX_EFFECTIVE_PERIODS + 1` so a
/// period run of `MAX_EFFECTIVE_PERIODS` fits as a date set.
pub const MAX_EFFECTIVE_DATES: usize = super::periods::MAX_EFFECTIVE_PERIODS + 1;

#[derive(Debug, errlanes::Rejection)]
#[rejection(code = "CALA_INVALID_EFFECTIVE_DATES")]
#[error("effective dates must be non-empty and within the work limit")]
pub struct InvalidEffectiveDates;

/// The effective dates at which cumulative balances are read (Twisp's
/// `effective.cumulative`, several at once). Stored sorted ascending and
/// deduplicated; dates are always explicit and the database clock is never
/// consulted.
#[derive(Debug, Clone)]
pub struct EffectiveDates {
    dates: Vec<NaiveDate>,
}

impl EffectiveDates {
    /// Sorts and deduplicates. Rejects an empty set or more than
    /// [`MAX_EFFECTIVE_DATES`] distinct dates.
    pub fn try_new(
        dates: impl IntoIterator<Item = NaiveDate>,
    ) -> Result<Self, InvalidEffectiveDates> {
        let mut dates: Vec<NaiveDate> = dates.into_iter().collect();
        dates.sort_unstable();
        dates.dedup();
        if dates.is_empty() || dates.len() > MAX_EFFECTIVE_DATES {
            return Err(InvalidEffectiveDates);
        }
        Ok(Self { dates })
    }

    /// The distinct dates, ascending.
    pub fn dates(&self) -> &[NaiveDate] {
        &self.dates
    }

    pub fn len(&self) -> usize {
        self.dates.len()
    }

    /// Always `false`: an empty set is rejected by [`Self::try_new`].
    pub fn is_empty(&self) -> bool {
        self.dates.is_empty()
    }

    pub fn first(&self) -> NaiveDate {
        self.dates[0]
    }

    pub fn last(&self) -> NaiveDate {
        self.dates[self.dates.len() - 1]
    }
}

#[derive(Debug, Clone)]
struct CumulativeEntry {
    date: NaiveDate,
    balance: AccountBalance,
    /// `None` when the pair had no row at or before `date` and `balance` is
    /// the zero fill.
    all_time_version: Option<u32>,
}

/// The cumulative balances of one balance at every requested date,
/// ascending. Exactly `dates.len()` entries.
#[derive(Debug, Clone)]
pub struct CumulativeBalances {
    entries: Vec<CumulativeEntry>,
}

impl CumulativeBalances {
    fn entry(&self, date: NaiveDate) -> Option<&CumulativeEntry> {
        self.entries
            .binary_search_by_key(&date, |entry| entry.date)
            .ok()
            .map(|index| &self.entries[index])
    }

    /// Cumulative balance as of `date`. `None` only when `date` was not
    /// requested.
    pub fn at(&self, date: NaiveDate) -> Option<&AccountBalance> {
        self.entry(date).map(|entry| &entry.balance)
    }

    /// `(date, balance)` for every requested date, ascending.
    pub fn iter(&self) -> impl Iterator<Item = (NaiveDate, &AccountBalance)> + '_ {
        self.entries
            .iter()
            .map(|entry| (entry.date, &entry.balance))
    }

    /// The range `[from, until]`, Twisp's `range` at day format: `open` is
    /// the balance as of `from - 1`, `close` the balance as of `until`,
    /// `period` their difference with `details.version` = entries in
    /// between. Identical to `find_all_in_range(ids, from, Some(until))` for
    /// a present id, including the zero `open` when the pair had no row
    /// before `from`. `None` only when `from - 1` or `until` was not
    /// requested (or `from` has no predecessor).
    pub fn range(&self, from: NaiveDate, until: NaiveDate) -> Option<BalanceRange> {
        let open = self.entry(from.pred_opt()?)?;
        let close = self.entry(until)?;
        let close_version = close.all_time_version.unwrap_or(0);
        Some(match open.all_time_version {
            Some(open_version) => BalanceRange::new(
                Some(open.balance.clone()),
                close.balance.clone(),
                close_version.saturating_sub(open_version),
            ),
            None => BalanceRange::new(None, close.balance.clone(), close_version),
        })
    }
}

/// A stored cumulative balance together with its `all_time_version`.
pub(crate) type Checkpoint = (AccountBalance, u32);

pub(crate) type Checkpoints = HashMap<(NaiveDate, BalanceId), Checkpoint>;

/// Folds the checkpoints fetched at `dates` for `id` (always a base
/// `BalanceId`; calculation checkpoints are keyed by their series' base id).
/// Each date carries the latest checkpoint at or before it forward; dates
/// before the first are zero-filled. `None` when the pair has no checkpoint
/// at any requested date.
pub(crate) fn fold_cumulative(
    id: BalanceId,
    dates: &EffectiveDates,
    checkpoints: &Checkpoints,
) -> Option<CumulativeBalances> {
    let checkpoint_at = |date: &NaiveDate| checkpoints.get(&(*date, id));
    let balance_type = dates
        .dates()
        .iter()
        .find_map(checkpoint_at)
        .map(|(balance, _)| balance.balance_type)?;

    let (journal_id, account_id, currency) = id;
    let mut current: Option<&Checkpoint> = None;
    let entries = dates
        .dates()
        .iter()
        .map(|date| {
            current = checkpoint_at(date).or(current);
            match current {
                Some((balance, version)) => CumulativeEntry {
                    date: *date,
                    balance: balance.clone(),
                    all_time_version: Some(*version),
                },
                None => CumulativeEntry {
                    date: *date,
                    balance: AccountBalance::zero(journal_id, account_id, currency, balance_type),
                    all_time_version: None,
                },
            }
        })
        .collect();
    Some(CumulativeBalances { entries })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::balance::snapshot::UNASSIGNED_ENTRY_ID;
    use cala_types::{
        balance::{BalanceAmount, BalanceSnapshot},
        primitives::{AccountId, Currency, DebitOrCredit, EntryId, JournalId},
    };
    use chrono::Utc;
    use rust_decimal::Decimal;

    fn date(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
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

    fn assert_range_eq(actual: &BalanceRange, expected: &BalanceRange) {
        assert_eq!(actual.open.details, expected.open.details);
        assert_eq!(actual.period.details, expected.period.details);
        assert_eq!(actual.close.details, expected.close.details);
        assert_eq!(actual.open.balance_type, expected.open.balance_type);
        assert_eq!(actual.period.balance_type, expected.period.balance_type);
        assert_eq!(actual.close.balance_type, expected.close.balance_type);
    }

    #[test]
    fn try_new_sorts_and_dedups() {
        let dates = EffectiveDates::try_new([
            date(2024, 3, 31),
            date(2023, 12, 31),
            date(2024, 3, 31),
            date(2024, 1, 31),
        ])
        .unwrap();
        assert_eq!(
            dates.dates(),
            &[date(2023, 12, 31), date(2024, 1, 31), date(2024, 3, 31)],
        );
        assert_eq!(dates.len(), 3);
        assert!(!dates.is_empty());
        assert_eq!(dates.first(), date(2023, 12, 31));
        assert_eq!(dates.last(), date(2024, 3, 31));
    }

    #[test]
    fn try_new_rejects_empty_and_too_many() {
        let jan_1 = date(2024, 1, 1);
        let days = |count: u64| (0..count).map(move |n| jan_1 + chrono::Days::new(n));
        assert!(EffectiveDates::try_new([]).is_err());
        assert!(EffectiveDates::try_new(days(402)).is_err());
        assert_eq!(EffectiveDates::try_new(days(401)).unwrap().len(), 401);
        assert_eq!(
            EffectiveDates::try_new(days(401).chain(days(401)))
                .unwrap()
                .len(),
            401,
        );
    }

    #[test]
    fn fold_cumulative_carries_forward_and_zero_fills() {
        let id = balance_id();
        let dates = EffectiveDates::try_new([
            date(2024, 1, 31),
            date(2024, 2, 29),
            date(2024, 3, 31),
            date(2024, 4, 30),
        ])
        .unwrap();
        let stored = checkpoint(id, 70, 3);
        let checkpoints: Checkpoints = HashMap::from([((date(2024, 2, 29), id), stored.clone())]);
        let cumulative = fold_cumulative(id, &dates, &checkpoints).expect("present");

        let entries: Vec<_> = cumulative.iter().collect();
        assert_eq!(entries.len(), 4);
        let (first_date, zero) = entries[0];
        assert_eq!(first_date, date(2024, 1, 31));
        assert_eq!(zero.settled(), Decimal::ZERO);
        assert_eq!(zero.details.version, 0);
        assert_eq!(zero.details.entry_id, EntryId::from(UNASSIGNED_ENTRY_ID));
        assert_eq!(zero.details.modified_at.timestamp(), 0);
        assert_eq!(zero.details.created_at.timestamp(), 0);
        assert_eq!(zero.balance_type, DebitOrCredit::Credit);
        assert_eq!(
            (
                zero.details.journal_id,
                zero.details.account_id,
                zero.details.currency
            ),
            id,
        );
        for (carried_date, carried) in &entries[1..] {
            assert_eq!(carried.details, stored.0.details, "{carried_date}");
        }
        assert_eq!(
            cumulative.at(date(2024, 4, 30)).unwrap().details,
            stored.0.details
        );
        assert!(cumulative.at(date(2024, 4, 29)).is_none());
    }

    #[test]
    fn fold_cumulative_without_rows_is_absent() {
        let id = balance_id();
        let dates = EffectiveDates::try_new([date(2024, 1, 31)]).unwrap();
        assert!(fold_cumulative(id, &dates, &HashMap::new()).is_none());
        let other = balance_id();
        let elsewhere: Checkpoints =
            HashMap::from([((date(2024, 1, 31), other), checkpoint(other, 5, 1))]);
        assert!(fold_cumulative(id, &dates, &elsewhere).is_none());
    }

    #[test]
    fn range_matches_balance_range_new() {
        let id = balance_id();
        let dates = EffectiveDates::try_new([
            date(2023, 12, 31),
            date(2024, 1, 17),
            date(2024, 2, 29),
            date(2024, 3, 31),
        ])
        .unwrap();
        let (open, open_version) = checkpoint(id, 120, 2);
        let (close, close_version) = checkpoint(id, 170, 5);
        let checkpoints: Checkpoints = HashMap::from([
            ((date(2024, 1, 17), id), (open.clone(), open_version)),
            ((date(2024, 2, 29), id), (close.clone(), close_version)),
        ]);
        let cumulative = fold_cumulative(id, &dates, &checkpoints).expect("present");

        let real_open = cumulative
            .range(date(2024, 1, 18), date(2024, 2, 29))
            .unwrap();
        assert_range_eq(
            &real_open,
            &BalanceRange::new(Some(open.clone()), close.clone(), 3),
        );
        assert_eq!(real_open.period.settled(), Decimal::from(50));
        assert_eq!(real_open.period.details.version, 3);

        let zero_open = cumulative
            .range(date(2024, 1, 1), date(2024, 2, 29))
            .unwrap();
        assert_range_eq(&zero_open, &BalanceRange::new(None, close.clone(), 5));

        let carried = cumulative
            .range(date(2024, 3, 1), date(2024, 3, 31))
            .unwrap();
        assert_range_eq(&carried, &BalanceRange::new(Some(close.clone()), close, 0));

        let all_zero = cumulative
            .range(date(2024, 1, 1), date(2023, 12, 31))
            .unwrap();
        for balance in [&all_zero.open, &all_zero.period, &all_zero.close] {
            assert_eq!(balance.settled(), Decimal::ZERO);
            assert_eq!(balance.details.version, 0);
        }

        assert!(cumulative
            .range(date(2024, 1, 2), date(2024, 2, 29))
            .is_none());
        assert!(cumulative
            .range(date(2024, 1, 1), date(2024, 2, 28))
            .is_none());
        assert!(cumulative
            .range(NaiveDate::MIN, date(2024, 2, 29))
            .is_none());
    }
}
