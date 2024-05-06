use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CalaOutboxImportConfig {
    pub endpoint: String,
}
