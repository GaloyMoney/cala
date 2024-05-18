use rust_decimal::Decimal;

use crate::primitives::*;
use cala_types::balance::*;

/// Representation of account's balance tracked in 3 distinct layers.
#[derive(Debug, Clone)]
pub struct AccountBalance {
    pub(super) balance_type: DebitOrCredit,
    pub details: BalanceSnapshot,
}

impl AccountBalance {
    pub fn pending(&self) -> Decimal {
        if self.balance_type == DebitOrCredit::Credit {
            self.details.pending_cr_balance - self.details.pending_dr_balance
        } else {
            self.details.pending_dr_balance - self.details.pending_cr_balance
        }
    }

    pub fn settled(&self) -> Decimal {
        if self.balance_type == DebitOrCredit::Credit {
            self.details.settled_cr_balance - self.details.settled_dr_balance
        } else {
            self.details.settled_dr_balance - self.details.settled_cr_balance
        }
    }

    pub fn encumbered(&self) -> Decimal {
        if self.balance_type == DebitOrCredit::Credit {
            self.details.encumbered_cr_balance - self.details.encumbered_dr_balance
        } else {
            self.details.encumbered_dr_balance - self.details.encumbered_cr_balance
        }
    }
}
