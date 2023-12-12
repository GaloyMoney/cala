use serde::{Deserialize, Serialize};

use super::primitives::*;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AccountValues {
    pub id: AccountId,
    pub code: String,
    pub name: String,
    pub normal_balance_type: DebitOrCredit,
    pub status: Status,
    pub external_id: Option<String>,
    pub description: Option<String>,
    pub tags: Vec<String>,
    pub metadata: Option<serde_json::Value>,
}
