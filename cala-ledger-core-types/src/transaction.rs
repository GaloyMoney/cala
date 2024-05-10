use serde::{Deserialize, Serialize};

use super::primitives::*;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TransactionValues {
    pub id: TransactionId,
    pub journal_id: JournalId,
    pub tx_template_id: TxTemplateId,
    pub effective: chrono::NaiveDate,
    pub correlation_id: CorrelationId,
    pub external_id: String,
    pub description: Option<String>,
    pub metadata: Option<serde_json::Value>,
}
