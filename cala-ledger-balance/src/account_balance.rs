use rust_decimal::Decimal;

use cala_types::balance::*;
use cala_types::primitives::*;

/// Representation of account's balance tracked in 3 distinct layers.
#[derive(Debug, Clone)]
pub struct AccountBalance {
    pub balance_type: DebitOrCredit,
    pub details: BalanceSnapshot,
}

impl AccountBalance {
    pub fn new(balance_type: DebitOrCredit, details: BalanceSnapshot) -> Self {
        Self {
            balance_type,
            details,
        }
    }

    pub(crate) fn derive_diff(mut self, since: &Self) -> Self {
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

#[derive(Debug, Clone)]
pub struct BalanceRange {
    pub open: AccountBalance,
    pub period: AccountBalance,
    pub close: AccountBalance,
}

impl BalanceRange {
    pub fn new(start: Option<AccountBalance>, end: AccountBalance, version_diff: u32) -> Self {
        match start {
            Some(start) => {
                let close = end.clone();
                let mut period = end.derive_diff(&start);
                period.details.version = version_diff;
                Self {
                    close,
                    period,
                    open: start,
                }
            }
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
                let mut range = end.clone();
                range.details.version = version_diff;
                Self {
                    period: range,
                    close: end.clone(),
                    open: AccountBalance {
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
