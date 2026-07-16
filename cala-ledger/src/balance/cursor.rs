use serde::{Deserialize, Serialize};

use cala_types::primitives::{AccountId, Currency, JournalId};

use super::{AccountBalance, BalanceRange};

#[derive(Debug, Serialize, Deserialize)]
pub struct AccountBalanceByCurrencyCursor {
    pub currency: Currency,
}

impl From<&AccountBalance> for AccountBalanceByCurrencyCursor {
    fn from(balance: &AccountBalance) -> Self {
        Self {
            currency: balance.details.currency,
        }
    }
}

impl From<&BalanceRange> for AccountBalanceByCurrencyCursor {
    fn from(range: &BalanceRange) -> Self {
        Self {
            currency: range.close.details.currency,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AccountBalanceCursor {
    pub journal_id: JournalId,
    pub account_id: AccountId,
    pub currency: Currency,
}

impl From<&AccountBalance> for AccountBalanceCursor {
    fn from(balance: &AccountBalance) -> Self {
        Self {
            journal_id: balance.details.journal_id,
            account_id: balance.details.account_id,
            currency: balance.details.currency,
        }
    }
}

impl From<&BalanceRange> for AccountBalanceCursor {
    fn from(range: &BalanceRange) -> Self {
        Self {
            journal_id: range.close.details.journal_id,
            account_id: range.close.details.account_id,
            currency: range.close.details.currency,
        }
    }
}
