use rust_decimal::Decimal;

use crate::primitives::*;
use cala_types::balance::*;

/// Representation of account's balance tracked in 3 distinct layers.
#[derive(Debug, Clone)]
pub struct AccountBalance {
    balance_type: DebitOrCredit,
    pub details: BalanceSnapshot,
}

impl AccountBalance {
    pub(crate) fn new(balance_type: DebitOrCredit, details: BalanceSnapshot) -> Self {
        Self {
            balance_type,
            details,
        }
    }

    pub(super) fn derive_diff(mut self, since: &Self) -> Self {
        self.details.settled = BalanceAmount {
            dr_balance: self.details.settled.dr_balance - since.details.settled.dr_balance,
            cr_balance: self.details.settled.cr_balance - since.details.settled.cr_balance,
            ..self.details.settled
        };
        self.details.pending = BalanceAmount {
            dr_balance: self.details.pending.dr_balance - since.details.pending.dr_balance,
            cr_balance: self.details.pending.cr_balance - since.details.pending.cr_balance,
            ..self.details.pending
        };
        self.details.encumbrance = BalanceAmount {
            dr_balance: self.details.encumbrance.dr_balance - since.details.encumbrance.dr_balance,
            cr_balance: self.details.encumbrance.cr_balance - since.details.encumbrance.cr_balance,
            ..self.details.encumbrance
        };
        self
    }

    pub fn pending(&self) -> Decimal {
        BalanceWithDirection {
            direction: self.balance_type,
            details: &self.details,
        }
        .pending()
    }

    pub fn settled(&self) -> Decimal {
        BalanceWithDirection {
            direction: self.balance_type,
            details: &self.details,
        }
        .settled()
    }

    pub fn encumbrance(&self) -> Decimal {
        BalanceWithDirection {
            direction: self.balance_type,
            details: &self.details,
        }
        .encumbrance()
    }

    pub fn available(&self, layer: Layer) -> Decimal {
        BalanceWithDirection {
            direction: self.balance_type,
            details: &self.details,
        }
        .available(layer)
    }
}

pub(crate) struct BalanceWithDirection<'a> {
    direction: DebitOrCredit,
    details: &'a BalanceSnapshot,
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

#[derive(Debug, Clone)]
pub struct BalanceRange {
    pub start: AccountBalance,
    pub diff: AccountBalance,
    pub end: AccountBalance,
}

impl BalanceRange {
    pub fn new(start: Option<AccountBalance>, end: AccountBalance) -> Self {
        match start {
            Some(start) => Self {
                end: end.clone(),
                diff: end.derive_diff(&start),
                start,
            },
            None => {
                use chrono::{TimeZone, Utc};
                let zero_time = Utc.timestamp_opt(0, 0).single().expect("0 timestamp");
                let zero_entry = EntryId::from(super::snapshot::UNASSIGNED_ENTRY_ID);
                let zero_amount = BalanceAmount {
                    dr_balance: Decimal::ZERO,
                    cr_balance: Decimal::ZERO,
                    entry_id: zero_entry,
                    modified_at: zero_time,
                };
                Self {
                    diff: end.clone(),
                    end: end.clone(),
                    start: AccountBalance {
                        balance_type: end.balance_type,
                        details: BalanceSnapshot {
                            version: 0,
                            created_at: zero_time,
                            modified_at: zero_time,
                            entry_id: zero_entry,
                            settled: zero_amount.clone(),
                            pending: zero_amount.clone(),
                            encumbrance: zero_amount,
                            ..end.details
                        },
                    },
                }
            }
        }
    }
}
