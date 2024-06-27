use cel_interpreter::{CelExpression, CelType, CelValue};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ParamDefinition {
    pub name: String,
    pub r#type: ParamDataType,
    pub default: Option<CelExpression>,
    pub description: Option<String>,
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

impl ParamDataType {
    pub fn coerce_value(&self, value: CelValue) -> Result<CelValue, String> {
        use cel_interpreter::CelType::*;
        match CelType::from(&value) {
            UInt if *self == ParamDataType::Integer => Ok(value),
            Int if *self == ParamDataType::Integer => Ok(value),
            String if *self == ParamDataType::String => Ok(value),
            Map if *self == ParamDataType::Json => Ok(value),
            Date if *self == ParamDataType::Date => Ok(value),
            Uuid if *self == ParamDataType::Uuid => Ok(value),
            Decimal if *self == ParamDataType::Decimal => Ok(value),
            Bool if *self == ParamDataType::Boolean => Ok(value),

            // Coercions
            String if *self == ParamDataType::Uuid => {
                if let CelValue::String(s) = value {
                    let uuid = s
                        .parse()
                        .map_err(|e| format!("Could not parse '{s}' as Uuid - {e}"))?;
                    Ok(CelValue::Uuid(uuid))
                } else {
                    unreachable!()
                }
            }
            String if *self == ParamDataType::Decimal => {
                if let CelValue::String(s) = value {
                    let decimal = s
                        .parse()
                        .map_err(|e| format!("Could not parse '{s}' as Decimal - {e}"))?;
                    Ok(CelValue::Decimal(decimal))
                } else {
                    unreachable!()
                }
            }
            String if *self == ParamDataType::Date => {
                if let CelValue::String(s) = value {
                    let date = s
                        .parse()
                        .map_err(|e| format!("Could not parse '{s}' as Date - {e}"))?;
                    Ok(CelValue::Date(date))
                } else {
                    unreachable!()
                }
            }
            _ => Err(format!("Type mismatch: expected {self:?}, got {value:?}")),
        }
    }
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
