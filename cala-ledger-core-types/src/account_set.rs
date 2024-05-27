use serde::{Deserialize, Serialize};

use super::primitives::*;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AccountSetValues {
    pub id: AccountSetId,
    pub version: u32,
    pub journal_id: JournalId,
    pub name: String,
    pub description: Option<String>,
    pub metadata: Option<serde_json::Value>,
    pub normal_balance_type: DebitOrCredit,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", content = "id")]
pub enum AccountSetMember {
    Account(AccountId),
    // AccountSet(AccountSetId),
}

impl From<AccountId> for AccountSetMember {
    fn from(account_id: AccountId) -> Self {
        AccountSetMember::Account(account_id)
    }
}
