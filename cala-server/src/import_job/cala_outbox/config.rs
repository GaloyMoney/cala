use cala_ledger_outbox_client::CalaLedgerOutboxClientConfig;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CalaOutboxImportConfig {
    pub endpoint: String,
}

impl From<&CalaOutboxImportConfig> for CalaLedgerOutboxClientConfig {
    fn from(config: &CalaOutboxImportConfig) -> Self {
        Self {
            url: config.endpoint.clone(),
        }
    }
}
