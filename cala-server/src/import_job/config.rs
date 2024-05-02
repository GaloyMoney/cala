use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ImportJobConfig {
    CalaOutbox(CalaOutboxImportConfig),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CalaOutboxImportConfig {
    pub endpoint: String,
}
