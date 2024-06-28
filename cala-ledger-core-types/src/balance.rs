use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use super::primitives::*;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BalanceSnapshot {
    pub journal_id: JournalId,
    pub account_id: AccountId,
    pub currency: Currency,
    pub version: u32,
    pub created_at: DateTime<Utc>,
    pub modified_at: DateTime<Utc>,
    pub entry_id: EntryId,
    pub settled: BalanceAmount,
    pub pending: BalanceAmount,
    pub encumbrance: BalanceAmount,
}

impl BalanceSnapshot {
    pub fn available(&self, layer: Layer) -> BalanceAmount {
        match layer {
            Layer::Settled => self.settled.clone(),
            Layer::Pending => self.settled.rollup(&self.pending),
            Layer::Encumbrance => self.settled.rollup(&self.pending).rollup(&self.encumbrance),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BalanceAmount {
    pub dr_balance: Decimal,
    pub cr_balance: Decimal,
    pub entry_id: EntryId,
    pub modified_at: DateTime<Utc>,
}

impl BalanceAmount {
    fn rollup(&self, other: &Self) -> Self {
        let (modified_at, entry_id) = if self.modified_at >= other.modified_at {
            (self.modified_at, self.entry_id)
        } else {
            (other.modified_at, other.entry_id)
        };

        Self {
            dr_balance: self.dr_balance + other.dr_balance,
            cr_balance: self.cr_balance + other.cr_balance,
            entry_id,
            modified_at,
        }
    }
}
