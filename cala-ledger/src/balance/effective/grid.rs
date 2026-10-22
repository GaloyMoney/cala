use std::collections::BTreeSet;

use chrono::NaiveDate;

use crate::primitives::BalanceId;

/// Maximum ranges in a grid, including overlapping/overall ranges.
pub const MAX_EFFECTIVE_BALANCE_RANGES: usize = 64;
/// Bounds the number of indexed snapshot probes, not just the returned rows.
pub const MAX_EFFECTIVE_BALANCE_GRID_WORK: usize = 1_000_000;

#[derive(Debug, es_entity::errlanes::Rejection)]
#[rejection(code = "CALA_INVALID_EFFECTIVE_BALANCE_GRID")]
#[error("effective balance grid has reversed dates or exceeds its work limit")]
pub struct InvalidEffectiveBalanceGrid;

/// A bounded account/currency/date read. Dates are inclusive and explicit;
/// no database wall clock is consulted. Result positions match `ranges`,
/// including duplicate ranges. Duplicate balance IDs are loaded only once.
#[derive(Debug)]
pub struct EffectiveBalanceGrid {
    pub(super) ids: Vec<BalanceId>,
    pub(super) ranges: Vec<(NaiveDate, NaiveDate)>,
    pub(super) boundaries: Vec<NaiveDate>,
}

impl EffectiveBalanceGrid {
    pub fn try_new(
        ids: &[BalanceId],
        ranges: &[(NaiveDate, NaiveDate)],
    ) -> Result<Self, InvalidEffectiveBalanceGrid> {
        if ranges.len() > MAX_EFFECTIVE_BALANCE_RANGES
            || ranges.iter().any(|(from, until)| from > until)
        {
            return Err(InvalidEffectiveBalanceGrid);
        }
        let mut seen = std::collections::HashSet::new();
        let ids: Vec<_> = ids.iter().copied().filter(|id| seen.insert(*id)).collect();
        let boundaries: Vec<_> = ranges
            .iter()
            .flat_map(|(from, until)| from.pred_opt().into_iter().chain(Some(*until)))
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        if ids
            .len()
            .checked_mul(boundaries.len())
            .is_none_or(|work| work > MAX_EFFECTIVE_BALANCE_GRID_WORK)
        {
            return Err(InvalidEffectiveBalanceGrid);
        }
        Ok(Self {
            ids,
            ranges: ranges.to_vec(),
            boundaries,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives::*;

    #[test]
    fn shares_adjacent_boundaries_and_deduplicates_ids() {
        let id = (JournalId::new(), AccountId::new(), Currency::USD);
        let ranges: Vec<_> = (1..=12)
            .map(|month| {
                let from = NaiveDate::from_ymd_opt(2024, month, 1).unwrap();
                let until = from
                    .checked_add_months(chrono::Months::new(1))
                    .unwrap()
                    .pred_opt()
                    .unwrap();
                (from, until)
            })
            .collect();
        let grid = EffectiveBalanceGrid::try_new(&[id, id], &ranges).unwrap();
        assert_eq!(grid.ids.len(), 1);
        assert_eq!(grid.boundaries.len(), 13);
    }

    #[test]
    fn validates_dates_and_bounds_work() {
        let date = NaiveDate::from_ymd_opt(2024, 1, 1).unwrap();
        assert!(EffectiveBalanceGrid::try_new(&[], &[(date, date.pred_opt().unwrap())]).is_err());
        assert!(EffectiveBalanceGrid::try_new(&[], &vec![(date, date); 65]).is_err());
        let ids: Vec<_> = (0..500_001)
            .map(|_| (JournalId::new(), AccountId::new(), Currency::USD))
            .collect();
        assert!(EffectiveBalanceGrid::try_new(&ids, &[(date, date)]).is_err());
        let grid = EffectiveBalanceGrid::try_new(&[], &[(NaiveDate::MIN, NaiveDate::MIN)]).unwrap();
        assert_eq!(grid.boundaries, vec![NaiveDate::MIN]);
    }
}
