use cel_interpreter::CelExpression;
use serde::{Deserialize, Serialize};

pub use super::param::*;
use super::primitives::*;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TxTemplateValues {
    pub id: TxTemplateId,
    pub version: u32,
    pub code: String,
    pub params: Option<Vec<ParamDefinition>>,
    pub tx_input: TxInput,
    pub entries: Vec<EntryInput>,
    pub description: Option<String>,
    pub metadata: Option<serde_json::Value>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EntryInput {
    pub entry_type: CelExpression,
    pub account_id: CelExpression,
    pub layer: CelExpression,
    pub direction: CelExpression,
    pub units: CelExpression,
    pub currency: CelExpression,
    pub description: Option<CelExpression>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TxInput {
    pub effective: CelExpression,
    pub journal_id: CelExpression,
    pub correlation_id: Option<CelExpression>,
    pub external_id: Option<CelExpression>,
    pub description: Option<CelExpression>,
    pub metadata: Option<CelExpression>,
}
