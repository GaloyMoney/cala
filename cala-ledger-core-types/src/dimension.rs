use cel_interpreter::CelExpression;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
pub struct PartitionKey {
    pub alias: String,
    pub value: CelExpression,
}

/// An evaluated dimension tuple: `{alias: value}` as JSON, the JSONB key column.
#[derive(Debug, Clone, sqlx::Type, PartialEq, Eq, Hash)]
#[sqlx(transparent)]
pub struct Window(serde_json::Value);

impl Window {
    pub fn empty() -> Self {
        Window(serde_json::json!({}))
    }

    pub fn inner(&self) -> &serde_json::Value {
        &self.0
    }
}

impl From<serde_json::Map<String, serde_json::Value>> for Window {
    fn from(map: serde_json::Map<String, serde_json::Value>) -> Self {
        Window(map.into())
    }
}

impl From<serde_json::Value> for Window {
    fn from(map: serde_json::Value) -> Self {
        Window(map)
    }
}
