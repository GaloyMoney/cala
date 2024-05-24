use serde::{Deserialize, Serialize};

use super::primitives::*;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TransactionValues {
    pub id: TransactionId,
    pub version: u32,
    pub journal_id: JournalId,
    pub tx_template_id: TxTemplateId,
    pub entry_ids: Vec<EntryId>,
    pub effective: chrono::NaiveDate,
    pub correlation_id: String,
    pub external_id: Option<String>,
    pub description: Option<String>,
    pub metadata: Option<serde_json::Value>,
}
