use cala_cel_interpreter::{CelExpression, CelType, CelValue};
use serde::{Deserialize, Serialize};

use super::primitives::*;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TxTemplateValues {
    pub id: TxTemplateId,
    pub code: String,
    pub description: Option<String>,
    pub params: Option<Vec<ParamDefinition>>,
    pub tx_input: TxInput,
    pub entries: Vec<EntryInput>,
    pub metadata: Option<serde_json::Value>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ParamDefinition {
    pub name: String,
    pub r#type: ParamDataType,
    pub default: Option<CelExpression>,
    pub description: Option<String>,
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
    pub effective: String,
    pub journal_id: String,
    pub correlation_id: Option<String>,
    pub external_id: Option<String>,
    pub description: Option<String>,
    pub metadata: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub enum ParamDataType {
    STRING,
    INTEGER,
    DECIMAL,
    BOOLEAN,
    UUID,
    DATE,
    TIMESTAMP,
    JSON,
}

// need a place to handle this
impl TryFrom<&CelValue> for ParamDataType {
    type Error = String;

    fn try_from(value: &CelValue) -> Result<Self, Self::Error> {
        use cala_cel_interpreter::CelType::*;
        match CelType::from(value) {
            Int => Ok(ParamDataType::INTEGER),
            String => Ok(ParamDataType::STRING),
            Map => Ok(ParamDataType::JSON),
            Date => Ok(ParamDataType::DATE),
            Uuid => Ok(ParamDataType::UUID),
            Decimal => Ok(ParamDataType::DECIMAL),
            Bool => Ok(ParamDataType::BOOLEAN),
            _ => Err(format!("Unsupported type: {value:?}")),
        }
    }
}
