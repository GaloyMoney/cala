use serde::{Deserialize, Serialize};

use super::cala_outbox::CalaOutboxImportConfig;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ImportJobConfig {
    CalaOutbox(CalaOutboxImportConfig),
}
