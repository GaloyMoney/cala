use serde::{Deserialize, Serialize};

use super::primitives::*;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AccountValues {
    pub id: AccountId,
    pub version: u32,
    pub code: String,
    pub name: String,
    pub normal_balance_type: DebitOrCredit,
    pub status: Status,
    pub external_id: Option<String>,
    pub description: Option<String>,
    pub metadata: Option<serde_json::Value>,
    pub config: AccountConfig,
}

#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct AccountConfig {
    pub is_account_set: bool,
    pub eventually_consistent: bool,
}
