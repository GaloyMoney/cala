use async_graphql::*;

use super::{convert::ToGlobalId, primitives::*};
use cala_ledger::primitives::*;

#[derive(SimpleObject)]
pub(super) struct Money {
    pub units: Decimal,
    pub currency: CurrencyCode,
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
    pub version: Int,
}

impl ToGlobalId for (JournalId, AccountId, Currency) {
    fn to_global_id(&self) -> async_graphql::types::ID {
        async_graphql::types::ID::from(format!("balance:{}:{}:{}", self.0, self.1, self.2))
    }
}

impl From<cala_ledger::balance::BalanceSnapshot> for Balance {
    fn from(balance: cala_ledger::balance::BalanceSnapshot) -> Self {
        unimplemented!()
        // Self {
        //     id: (balance.journal_id, balance.account_id, balance.currency).to_global_id(),
        //     journal_id: balance.journal_id.into(),
        //     account_id: balance.account_id.into(),
        //     entry_id: balance.entry_id.into(),
        //     currency: balance.currency.into(),
        //     settled: BalanceAmount {
        // dr_balance: balance.settled_dr_balance.into(),
        // cr_balance: balance.settled_cr_balance.into(),
        // // normal_balance:
        // entry_id: UUID,
        //     }
        //     pending: balance.pending.into(),
        //     encumbrance: balance.encumbrance.into(),
        //     version: balance.version.into(),
        // }
    }
}
