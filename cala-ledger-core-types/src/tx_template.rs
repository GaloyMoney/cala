use cel_interpreter::CelExpression;
use serde::{Deserialize, Serialize};

pub use crate::param::*;
use crate::primitives::*;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
pub struct TxTemplateValues {
    pub id: TxTemplateId,
    pub version: u32,
    pub code: String,
    pub params: Option<Vec<ParamDefinition>>,
    pub transaction: TxTemplateTransaction,
    pub entries: Vec<TxTemplateEntry>,
    pub description: Option<String>,
    pub metadata: Option<serde_json::Value>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
pub struct TxTemplateEntry {
    pub entry_type: CelExpression,
    pub account_id: CelExpression,
    pub layer: CelExpression,
    pub direction: CelExpression,
    pub units: CelExpression,
    pub currency: CelExpression,
    pub description: Option<CelExpression>,
    pub metadata: Option<CelExpression>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
pub struct TxTemplateTransaction {
    pub effective: CelExpression,
    pub journal_id: CelExpression,
    pub correlation_id: Option<CelExpression>,
    pub external_id: Option<CelExpression>,
    pub description: Option<CelExpression>,
    pub metadata: Option<CelExpression>,
}
