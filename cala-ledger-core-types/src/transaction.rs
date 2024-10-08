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

mod cel {
    use cel_interpreter::{CelMap, CelValue};

    impl From<&super::TransactionValues> for CelValue {
        fn from(tx: &super::TransactionValues) -> Self {
            let mut map = CelMap::new();
            map.insert("id", tx.id);
            map.insert("journalId", tx.journal_id);
            map.insert("txTemplateId", tx.tx_template_id);
            map.insert("effective", tx.effective);
            map.insert("correlationId", tx.correlation_id.clone());
            if let Some(metadata) = &tx.metadata {
                map.insert("metadata", metadata.clone());
            }
            map.into()
        }
    }
}
