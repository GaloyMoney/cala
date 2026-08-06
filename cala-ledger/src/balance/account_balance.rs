use rust_decimal::Decimal;

use crate::primitives::*;
use cala_types::balance::*;

/// Representation of account's balance tracked in 3 distinct layers.
#[derive(Debug, Clone)]
pub struct AccountBalance {
    pub balance_type: DebitOrCredit,
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

#[cfg(test)]
mod tests {
    use chrono::Utc;

    use super::*;

    fn account_balance() -> AccountBalance {
        let entry_id = EntryId::new();
        let time = Utc::now();
        let amount = BalanceAmount {
            dr_balance: Decimal::ZERO,
            cr_balance: Decimal::ZERO,
            entry_id,
            modified_at: time,
        };
        AccountBalance::new(
            DebitOrCredit::Credit,
            BalanceSnapshot {
                journal_id: JournalId::new(),
                account_id: AccountId::new(),
                entry_id,
                currency: Currency::USD,
                settled: amount.clone(),
                pending: amount.clone(),
                encumbrance: amount,
                version: 0,
                modified_at: time,
                created_at: time,
            },
        )
    }

    #[test]
    fn from_bounds_sets_period_version_to_the_diff() {
        let range =
            BalanceRange::from_bounds(Some(account_balance()), 3, Some(account_balance()), 10)
                .expect("a close balance yields a range");
        assert_eq!(range.period.details.version, 7);
    }

    #[test]
    fn from_bounds_without_close_is_none() {
        assert!(BalanceRange::from_bounds(Some(account_balance()), 3, None, 10).is_none());
        assert!(BalanceRange::from_bounds(None, 0, None, 0).is_none());
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

    /// Build a range from its `(open, close)` bounds. Returns `None` when
    /// there is no closing balance — i.e. the account had no activity in
    /// the window, so there is no range to report.
    pub fn from_bounds(
        start: Option<AccountBalance>,
        start_version: u32,
        end: Option<AccountBalance>,
        end_version: u32,
    ) -> Option<Self> {
        end.map(|end| Self::new(start, end, end_version - start_version))
    }
}
