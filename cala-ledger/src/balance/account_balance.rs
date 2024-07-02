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
    pub(super) fn derive_as_of(mut self, as_of: Self) -> Self {
        self.details.settled = BalanceAmount {
            dr_balance: self.details.settled.dr_balance - as_of.details.settled.dr_balance,
            cr_balance: self.details.settled.cr_balance - as_of.details.settled.cr_balance,
            ..self.details.settled
        };
        self.details.pending = BalanceAmount {
            dr_balance: self.details.pending.dr_balance - as_of.details.pending.dr_balance,
            cr_balance: self.details.pending.cr_balance - as_of.details.pending.cr_balance,
            ..self.details.pending
        };
        self.details.encumbrance = BalanceAmount {
            dr_balance: self.details.encumbrance.dr_balance - as_of.details.encumbrance.dr_balance,
            cr_balance: self.details.encumbrance.cr_balance - as_of.details.encumbrance.cr_balance,
            ..self.details.encumbrance
        };
        self
    }

    pub fn pending(&self) -> Decimal {
        if self.balance_type == DebitOrCredit::Credit {
            self.details.pending.cr_balance - self.details.pending.dr_balance
        } else {
            self.details.pending.dr_balance - self.details.pending.cr_balance
        }
    }

    pub fn settled(&self) -> Decimal {
        if self.balance_type == DebitOrCredit::Credit {
            self.details.settled.cr_balance - self.details.settled.dr_balance
        } else {
            self.details.settled.dr_balance - self.details.settled.cr_balance
        }
    }

    pub fn encumbrance(&self) -> Decimal {
        if self.balance_type == DebitOrCredit::Credit {
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
