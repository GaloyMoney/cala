//! Per-`(series, effective date)` deltas.
//!
//! A batch of entries is folded in the application into one [`DateDelta`]
//! per `(series, effective date)`. Only these deltas — never any stored
//! balance row — cross the wire to Postgres, which adds them to the
//! affected rows in a single set-based statement (each table's
//! `apply_deltas_in_op`).

use chrono::NaiveDate;
use rust_decimal::Decimal;

use std::{collections::HashMap, hash::Hash};

use cala_types::{
    entry::EntryValues,
    primitives::{AccountId, Currency, DebitOrCredit, EntryId, Layer},
};

use crate::balance::UNASSIGNED_ENTRY_ID;

/// Position of an entry in the batch: transaction landing order, then the
/// entry's sequence within its transaction. The greatest position is the
/// "last" entry.
type EntryPosition = (usize, u32);

/// Summed amounts and last entry of one layer within a [`DateDelta`].
#[derive(Debug, Clone)]
pub(crate) struct LayerDelta {
    pub dr_balance: Decimal,
    pub cr_balance: Decimal,
    /// Last entry of this layer; the nil UUID when no entry touched it.
    pub entry_id: EntryId,
    last_position: Option<EntryPosition>,
}

impl LayerDelta {
    fn new() -> Self {
        Self {
            dr_balance: Decimal::ZERO,
            cr_balance: Decimal::ZERO,
            entry_id: EntryId::from(UNASSIGNED_ENTRY_ID),
            last_position: None,
        }
    }

    fn push(&mut self, position: EntryPosition, entry: &EntryValues) {
        match entry.direction {
            DebitOrCredit::Debit => self.dr_balance += entry.units,
            DebitOrCredit::Credit => self.cr_balance += entry.units,
        }
        if self.last_position.is_none_or(|last| position >= last) {
            self.last_position = Some(position);
            self.entry_id = entry.id;
        }
    }
}

/// Everything one batch adds to one series on one effective date.
///
/// A series is identified by `(series, account_id, currency)`. `series` is
/// the part of that key beyond the account and currency: `()` for the
/// account table, which has exactly one series per pair, and
/// `(CalculationId, Window)` for the calculation table.
#[derive(Debug, Clone)]
pub(crate) struct DateDelta<K = ()> {
    pub series: K,
    pub account_id: AccountId,
    pub currency: Currency,
    pub effective: NaiveDate,
    /// Number of entries folded.
    pub entries: u32,
    /// Last entry folded, over all layers.
    pub last_entry_id: EntryId,
    last_position: EntryPosition,
    pub settled: LayerDelta,
    pub pending: LayerDelta,
    pub encumbrance: LayerDelta,
}

/// Accumulates a batch's entries into [`DateDelta`]s, one per
/// `(series, account, currency, effective date)`.
#[derive(Debug)]
pub(crate) struct DeltaAccumulator<K = ()> {
    deltas: HashMap<(K, AccountId, Currency, NaiveDate), DateDelta<K>>,
}

impl<K> Default for DeltaAccumulator<K> {
    fn default() -> Self {
        Self {
            deltas: HashMap::new(),
        }
    }
}

impl DeltaAccumulator<()> {
    /// Fold `entry`, belonging to the `tx_index`-th transaction of the batch
    /// and dated `effective`, into the delta of `account_id` (the entry's own
    /// account or one of its ancestor sets).
    pub fn push(
        &mut self,
        account_id: AccountId,
        effective: NaiveDate,
        tx_index: usize,
        entry: &EntryValues,
    ) {
        self.push_keyed((), account_id, effective, tx_index, entry)
    }
}

impl<K: Clone + Eq + Hash> DeltaAccumulator<K> {
    pub fn new() -> Self {
        Self::default()
    }

    /// Like `push`, within the `series` of `account_id`.
    pub fn push_keyed(
        &mut self,
        series: K,
        account_id: AccountId,
        effective: NaiveDate,
        tx_index: usize,
        entry: &EntryValues,
    ) {
        let position = (tx_index, entry.sequence);
        let delta = self
            .deltas
            .entry((series.clone(), account_id, entry.currency, effective))
            .or_insert_with(|| DateDelta {
                series,
                account_id,
                currency: entry.currency,
                effective,
                entries: 0,
                last_entry_id: entry.id,
                last_position: position,
                settled: LayerDelta::new(),
                pending: LayerDelta::new(),
                encumbrance: LayerDelta::new(),
            });
        delta.entries += 1;
        if position >= delta.last_position {
            delta.last_position = position;
            delta.last_entry_id = entry.id;
        }
        match entry.layer {
            Layer::Settled => delta.settled.push(position, entry),
            Layer::Pending => delta.pending.push(position, entry),
            Layer::Encumbrance => delta.encumbrance.push(position, entry),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.deltas.is_empty()
    }

    pub fn into_deltas(self) -> Vec<DateDelta<K>> {
        self.deltas.into_values().collect()
    }
}

#[cfg(test)]
mod tests {
    use rust_decimal::Decimal;

    use super::*;

    fn entry(account_id: AccountId, sequence: u32, units: i64) -> EntryValues {
        EntryValues {
            id: EntryId::new(),
            version: 1,
            transaction_id: cala_types::primitives::TransactionId::new(),
            journal_id: cala_types::primitives::JournalId::new(),
            account_id,
            entry_type: "TEST".to_string(),
            sequence,
            layer: Layer::Settled,
            currency: "USD".parse().unwrap(),
            direction: DebitOrCredit::Credit,
            units: Decimal::from(units),
            description: None,
            metadata: None,
        }
    }

    #[test]
    fn push_keyed_with_distinct_keys_yields_two_deltas() {
        let account_id = AccountId::new();
        let date = NaiveDate::from_ymd_opt(2026, 1, 15).unwrap();
        let mut deltas = DeltaAccumulator::<&str>::new();
        deltas.push_keyed("a", account_id, date, 0, &entry(account_id, 1, 10));
        deltas.push_keyed("b", account_id, date, 0, &entry(account_id, 2, 5));
        deltas.push_keyed("a", account_id, date, 0, &entry(account_id, 3, 7));

        let mut deltas = deltas.into_deltas();
        deltas.sort_by_key(|d| d.series);
        assert_eq!(deltas.len(), 2);
        assert_eq!(deltas[0].series, "a");
        assert_eq!(deltas[0].entries, 2);
        assert_eq!(deltas[0].settled.cr_balance, Decimal::from(17));
        assert_eq!(deltas[1].series, "b");
        assert_eq!(deltas[1].entries, 1);
        assert_eq!(deltas[1].settled.cr_balance, Decimal::from(5));
    }

    #[test]
    fn push_on_unkeyed_accumulator_folds_into_one_delta() {
        let account_id = AccountId::new();
        let date = NaiveDate::from_ymd_opt(2026, 1, 15).unwrap();
        let first = entry(account_id, 1, 10);
        let second = entry(account_id, 2, 5);
        let mut deltas = DeltaAccumulator::new();
        deltas.push(account_id, date, 0, &first);
        deltas.push(account_id, date, 0, &second);

        let deltas = deltas.into_deltas();
        assert_eq!(deltas.len(), 1);
        assert_eq!(deltas[0].entries, 2);
        assert_eq!(deltas[0].settled.cr_balance, Decimal::from(15));
        assert_eq!(deltas[0].last_entry_id, second.id);
    }
}
