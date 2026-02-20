mod traits;

pub use traits::*;

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use super::entry::EntryValues;
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

pub const UNASSIGNED_ENTRY_ID: uuid::Uuid = uuid::Uuid::nil();

pub struct Snapshots;

impl Snapshots {
    pub fn new_snapshot(
        time: DateTime<Utc>,
        account_id: AccountId,
        entry: &EntryValues,
    ) -> BalanceSnapshot {
        let entry_id = EntryId::from(UNASSIGNED_ENTRY_ID);
        Self::update_snapshot(
            time,
            BalanceSnapshot {
                journal_id: entry.journal_id,
                account_id,
                entry_id,
                currency: entry.currency,
                settled: BalanceAmount {
                    dr_balance: Decimal::ZERO,
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
                version: 0,
                modified_at: time,
                created_at: time,
            },
            entry,
        )
    }

    pub fn update_snapshot(
        time: DateTime<Utc>,
        mut snapshot: BalanceSnapshot,
        entry: &EntryValues,
    ) -> BalanceSnapshot {
        snapshot.version += 1;
        snapshot.modified_at = time;
        snapshot.entry_id = entry.id;
        match entry.layer {
            Layer::Settled => {
                snapshot.settled.entry_id = entry.id;
                snapshot.settled.modified_at = time;
                match entry.direction {
                    DebitOrCredit::Debit => {
                        snapshot.settled.dr_balance += entry.units;
                    }
                    DebitOrCredit::Credit => {
                        snapshot.settled.cr_balance += entry.units;
                    }
                }
            }
            Layer::Pending => {
                snapshot.pending.entry_id = entry.id;
                snapshot.pending.modified_at = time;
                match entry.direction {
                    DebitOrCredit::Debit => {
                        snapshot.pending.dr_balance += entry.units;
                    }
                    DebitOrCredit::Credit => {
                        snapshot.pending.cr_balance += entry.units;
                    }
                }
            }
            Layer::Encumbrance => {
                snapshot.encumbrance.entry_id = entry.id;
                snapshot.encumbrance.modified_at = time;
                match entry.direction {
                    DebitOrCredit::Debit => {
                        snapshot.encumbrance.dr_balance += entry.units;
                    }
                    DebitOrCredit::Credit => {
                        snapshot.encumbrance.cr_balance += entry.units;
                    }
                }
            }
        }
        snapshot
    }
}

pub struct BalanceWithDirection<'a> {
    pub direction: DebitOrCredit,
    pub details: &'a BalanceSnapshot,
}

impl<'a> BalanceWithDirection<'a> {
    pub fn new(direction: DebitOrCredit, details: &'a BalanceSnapshot) -> Self {
        Self { direction, details }
    }

    pub fn pending(&self) -> Decimal {
        if self.direction == DebitOrCredit::Credit {
            self.details.pending.cr_balance - self.details.pending.dr_balance
        } else {
            self.details.pending.dr_balance - self.details.pending.cr_balance
        }
    }

    pub fn settled(&self) -> Decimal {
        if self.direction == DebitOrCredit::Credit {
            self.details.settled.cr_balance - self.details.settled.dr_balance
        } else {
            self.details.settled.dr_balance - self.details.settled.cr_balance
        }
    }

    pub fn encumbrance(&self) -> Decimal {
        if self.direction == DebitOrCredit::Credit {
            self.details.encumbrance.cr_balance - self.details.encumbrance.dr_balance
        } else {
            self.details.encumbrance.dr_balance - self.details.encumbrance.cr_balance
        }
    }

    pub fn available(&self, layer: Layer) -> Decimal {
        match layer {
            Layer::Settled => self.settled(),
            Layer::Pending => self.pending() + self.settled(),
            Layer::Encumbrance => self.encumbrance() + self.pending() + self.settled(),
        }
    }
}
