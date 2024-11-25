use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct CalaLedgerOutboxClientConfig {
    pub url: String,
}

impl CalaLedgerOutboxClientConfig {
    pub fn new(url: String) -> Self {
        Self { url }
    }
}
