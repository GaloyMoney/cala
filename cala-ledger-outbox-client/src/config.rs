use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct CalaLedgerOutboxClientConfig {
    pub url: String,
}
