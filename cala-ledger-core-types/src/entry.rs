use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use super::primitives::*;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EntryValues {
    pub id: EntryId,
    pub version: u32,
    pub transaction_id: TransactionId,
    pub journal_id: JournalId,
    pub account_id: AccountId,
    pub entry_type: String,
    pub sequence: u32,
    pub layer: Layer,
    pub units: Decimal,
    pub currency: Currency,
    pub direction: DebitOrCredit,
    pub description: Option<String>,
    pub metadata: Option<serde_json::Value>,
}

mod cel {
    use cel_interpreter::{CelMap, CelValue};

    impl From<&super::EntryValues> for CelValue {
        fn from(entry: &super::EntryValues) -> Self {
            let mut map = CelMap::new();
            map.insert("id", entry.id);
            map.insert("entryType", entry.entry_type.clone());
            map.insert("sequence", CelValue::UInt(entry.sequence as u64));
            map.insert("layer", entry.layer);
            map.insert("direction", entry.direction);
            map.insert("units", entry.units);
            map.insert("currency", entry.currency);
            if let Some(metadata) = &entry.metadata {
                map.insert("metadata", metadata.clone());
            }
            map.into()
        }
    }
}
