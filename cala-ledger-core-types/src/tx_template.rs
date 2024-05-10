use cel_interpreter::{CelExpression, CelType, CelValue};
use serde::{Deserialize, Serialize};

use super::primitives::*;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TxTemplateValues {
    pub id: TxTemplateId,
    pub code: String,
    pub params: Option<Vec<ParamDefinition>>,
    pub tx_input: TxInput,
    pub entries: Vec<EntryInput>,
    pub description: Option<String>,
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
    pub effective: CelExpression,
    pub journal_id: CelExpression,
    pub correlation_id: Option<CelExpression>,
    pub external_id: Option<CelExpression>,
    pub description: Option<CelExpression>,
    pub metadata: Option<CelExpression>,
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

impl TryFrom<&CelValue> for ParamDataType {
    type Error = String;

    fn try_from(value: &CelValue) -> Result<Self, Self::Error> {
        use cel_interpreter::CelType::*;
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
