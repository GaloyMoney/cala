use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::primitives::*;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AccountSetValues {
    pub id: AccountSetId,
    pub version: u32,
    pub journal_id: JournalId,
    pub name: String,
    pub external_id: Option<String>,
    pub description: Option<String>,
    pub metadata: Option<serde_json::Value>,
    pub normal_balance_type: DebitOrCredit,
}

#[derive(Clone, Debug)]
pub struct AccountSetMember {
    pub id: AccountSetMemberId,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", content = "id")]
pub enum AccountSetMemberId {
    Account(AccountId),
    AccountSet(AccountSetId),
}

impl From<AccountId> for AccountSetMemberId {
    fn from(account_id: AccountId) -> Self {
        Self::Account(account_id)
    }
}

impl From<AccountSetId> for AccountSetMemberId {
    fn from(id: AccountSetId) -> Self {
        Self::AccountSet(id)
    }
}

impl From<(AccountSetMemberId, DateTime<Utc>)> for AccountSetMember {
    fn from((id, created_at): (AccountSetMemberId, DateTime<Utc>)) -> Self {
        Self { id, created_at }
    }
}
