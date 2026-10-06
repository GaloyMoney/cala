//! Per-`(pair, effective date)` deltas.
//!
//! A batch of entries is folded in the application into one [`DateDelta`]
//! per `(account, currency, effective date)`. Only these deltas — never any
//! stored balance row — cross the wire to Postgres, which adds them to the
//! affected rows in a single set-based statement
//! (`EffectiveBalanceRepo::apply_deltas_in_op`).

use chrono::NaiveDate;
use rust_decimal::Decimal;

use std::collections::HashMap;

use cala_types::{
    entry::EntryValues,
    primitives::{AccountId, Currency, DebitOrCredit, EntryId, Layer},
};

use crate::balance::snapshot::UNASSIGNED_ENTRY_ID;

/// Position of an entry in the batch: transaction landing order, then the
/// entry's sequence within its transaction. The greatest position is the
/// "last" entry.
type EntryPosition = (usize, u32);

/// Summed amounts and last entry of one layer within a [`DateDelta`].
#[derive(Debug, Clone)]
pub(super) struct LayerDelta {
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

/// Everything one batch adds to one pair on one effective date.
#[derive(Debug, Clone)]
pub(super) struct DateDelta {
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

/// Accumulates a batch's entries into [`DateDelta`]s.
#[derive(Debug, Default)]
pub(super) struct DeltaAccumulator {
    deltas: HashMap<(AccountId, Currency, NaiveDate), DateDelta>,
}

impl DeltaAccumulator {
    pub fn new() -> Self {
        Self::default()
    }

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
        let position = (tx_index, entry.sequence);
        let delta = self
            .deltas
            .entry((account_id, entry.currency, effective))
            .or_insert_with(|| DateDelta {
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

    pub fn into_deltas(self) -> Vec<DateDelta> {
        self.deltas.into_values().collect()
    }
}
