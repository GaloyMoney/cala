use async_graphql::*;

use super::{convert::ToGlobalId, primitives::*};
use cala_ledger::primitives::*;

#[derive(SimpleObject)]
pub(super) struct Money {
    pub units: Decimal,
    pub currency: CurrencyCode,
}

impl From<(rust_decimal::Decimal, Currency)> for Money {
    fn from((units, currency): (rust_decimal::Decimal, Currency)) -> Self {
        Self {
            units: units.into(),
            currency: currency.into(),
        }
    }
}

#[derive(SimpleObject)]
pub(super) struct BalanceAmount {
    pub dr_balance: Money,
    pub cr_balance: Money,
    pub normal_balance: Money,
    pub entry_id: UUID,
}

#[derive(SimpleObject)]
pub(super) struct Balance {
    pub id: ID,
    pub journal_id: UUID,
    pub account_id: UUID,
    pub entry_id: UUID,
    pub currency: CurrencyCode,
    pub settled: BalanceAmount,
    pub pending: BalanceAmount,
    pub encumbrance: BalanceAmount,
    pub version: u32,
}

impl ToGlobalId for (JournalId, AccountId, Currency) {
    fn to_global_id(&self) -> async_graphql::types::ID {
        async_graphql::types::ID::from(format!("balance:{}:{}:{}", self.0, self.1, self.2))
    }
}

impl From<cala_ledger::balance::AccountBalance> for Balance {
    fn from(balance: cala_ledger::balance::AccountBalance) -> Self {
        let currency = balance.details.currency;
        Self {
            id: (
                balance.details.journal_id,
                balance.details.account_id,
                balance.details.currency,
            )
                .to_global_id(),
            journal_id: balance.details.journal_id.into(),
            account_id: balance.details.account_id.into(),
            entry_id: balance.details.entry_id.into(),
            currency: balance.details.currency.into(),
            version: balance.details.version,
            settled: BalanceAmount {
                dr_balance: (balance.details.settled.dr_balance, currency).into(),
                cr_balance: (balance.details.settled.cr_balance, currency).into(),
                normal_balance: (balance.settled(), currency).into(),
                entry_id: balance.details.settled.entry_id.into(),
            },
            pending: BalanceAmount {
                dr_balance: (balance.details.pending.dr_balance, currency).into(),
                cr_balance: (balance.details.pending.cr_balance, currency).into(),
                normal_balance: (balance.pending(), currency).into(),
                entry_id: balance.details.pending.entry_id.into(),
            },
            encumbrance: BalanceAmount {
                dr_balance: (balance.details.encumbrance.dr_balance, currency).into(),
                cr_balance: (balance.details.encumbrance.cr_balance, currency).into(),
                normal_balance: (balance.encumbrance(), currency).into(),
                entry_id: balance.details.encumbrance.entry_id.into(),
            },
        }
    }
}
