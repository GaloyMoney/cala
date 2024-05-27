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
