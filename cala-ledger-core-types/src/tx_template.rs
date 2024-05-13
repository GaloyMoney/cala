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
    String,
    Integer,
    Decimal,
    Boolean,
    Uuid,
    Date,
    Timestamp,
    Json,
}

impl TryFrom<&CelValue> for ParamDataType {
    type Error = String;

    fn try_from(value: &CelValue) -> Result<Self, Self::Error> {
        use cel_interpreter::CelType::*;
        match CelType::from(value) {
            Int => Ok(ParamDataType::Integer),
            String => Ok(ParamDataType::String),
            Map => Ok(ParamDataType::Json),
            Date => Ok(ParamDataType::Date),
            Uuid => Ok(ParamDataType::Uuid),
            Decimal => Ok(ParamDataType::Decimal),
            Bool => Ok(ParamDataType::Boolean),
            _ => Err(format!("Unsupported type: {value:?}")),
        }
    }
}
