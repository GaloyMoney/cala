use serde::{Deserialize, Serialize};

use super::primitives::*;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
pub struct JournalValues {
    pub id: JournalId,
    pub version: u32,
    pub name: String,
    pub code: Option<String>,
    pub status: Status,
    pub description: Option<String>,
    pub config: JournalConfig,
}

#[derive(Default, Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
pub struct JournalConfig {
    pub enable_effective_balances: bool,
}
