use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

use cala_types::{
    balance::EffectiveBalanceSnapshot,
    primitives::{AccountId, Currency},
};

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
    pub account_id: AccountId,
    pub currency: Currency,
}

impl From<&AccountBalance> for AccountBalanceCursor {
    fn from(balance: &AccountBalance) -> Self {
        Self {
            account_id: balance.details.account_id,
            currency: balance.details.currency,
        }
    }
}

impl From<&BalanceRange> for AccountBalanceCursor {
    fn from(range: &BalanceRange) -> Self {
        Self {
            account_id: range.close.details.account_id,
            currency: range.close.details.currency,
        }
    }
}

/// Keyset cursor for [`super::EffectiveBalances::list_modified_since`].
/// Orders on the tuple identity itself — `(account_id, currency, effective)`
/// — which doubles as the `DISTINCT ON` grouping key in the backing query,
/// so pagination stays stable regardless of how many times a tuple was
/// rewritten (backdating fan-out) between pages.
#[derive(Debug, Serialize, Deserialize)]
pub struct EffectiveBalancesModifiedCursor {
    pub account_id: AccountId,
    pub currency: Currency,
    pub effective: NaiveDate,
}

impl From<&EffectiveBalanceSnapshot> for EffectiveBalancesModifiedCursor {
    fn from(snapshot: &EffectiveBalanceSnapshot) -> Self {
        Self {
            account_id: snapshot.account_id,
            currency: snapshot.currency,
            effective: snapshot.effective,
        }
    }
}
