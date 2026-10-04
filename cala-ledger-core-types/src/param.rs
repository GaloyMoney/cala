use cel_interpreter::{CelExpression, CelType, CelValue};
use es_entity::errlanes;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
pub struct ParamDefinition {
    pub name: String,
    pub r#type: ParamDataType,
    pub default: Option<CelExpression>,
    pub description: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
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
    pub fn coerce_value(&self, value: CelValue) -> Result<CelValue, ParamValueRejection> {
        use cel_interpreter::CelType::*;
        match CelType::from(&value) {
            UInt if *self == ParamDataType::Integer => Ok(value),
            Int if *self == ParamDataType::Integer => Ok(value),
            String if *self == ParamDataType::String => Ok(value),
            Map if *self == ParamDataType::Json => Ok(value),
            Date if *self == ParamDataType::Date => Ok(value),
            Timestamp if *self == ParamDataType::Date => {
                if let CelValue::Timestamp(ts) = value {
                    Ok(CelValue::Date(ts.date_naive()))
                } else {
                    unreachable!()
                }
            }
            Uuid if *self == ParamDataType::Uuid => Ok(value),
            Decimal if *self == ParamDataType::Decimal => Ok(value),
            Bool if *self == ParamDataType::Boolean => Ok(value),

            // Coercions
            String if *self == ParamDataType::Uuid => {
                if let CelValue::String(s) = value {
                    let uuid = s.parse().map_err(|e| ParamValueRejection::InvalidUuid {
                        input: s.to_string(),
                        source: e,
                    })?;
                    Ok(CelValue::Uuid(uuid))
                } else {
                    unreachable!()
                }
            }
            String if *self == ParamDataType::Decimal => {
                if let CelValue::String(s) = value {
                    let decimal = s.parse().map_err(|e| ParamValueRejection::InvalidDecimal {
                        input: s.to_string(),
                        source: e,
                    })?;
                    Ok(CelValue::Decimal(decimal))
                } else {
                    unreachable!()
                }
            }
            String if *self == ParamDataType::Date => {
                if let CelValue::String(s) = value {
                    let date = s.parse().map_err(|e| ParamValueRejection::InvalidDate {
                        input: s.to_string(),
                        source: e,
                    })?;
                    Ok(CelValue::Date(date))
                } else {
                    unreachable!()
                }
            }
            _ => Err(ParamValueRejection::TypeMismatch {
                expected: self.clone(),
                actual: CelType::from(&value),
            }),
        }
    }
}

impl TryFrom<&CelValue> for ParamDataType {
    type Error = UnsupportedParamType;

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
            _ => Err(UnsupportedParamType(CelType::from(value))),
        }
    }
}

#[derive(Debug, errlanes::Rejection)]
pub enum ParamValueRejection {
    #[rejection(code = "PARAM_TYPE_MISMATCH")]
    #[error("Type mismatch: expected {:?}, got {:?}", expected, actual)]
    TypeMismatch {
        expected: ParamDataType,
        actual: CelType,
    },
    #[rejection(code = "PARAM_TYPE_MISMATCH")]
    #[error("Could not parse {} as Uuid: {}", input, source)]
    InvalidUuid {
        input: String,
        #[source]
        source: uuid::Error,
    },
    #[rejection(code = "PARAM_TYPE_MISMATCH")]
    #[error("Could not parse {} as Decimal: {}", input, source)]
    InvalidDecimal {
        input: String,
        #[source]
        source: rust_decimal::Error,
    },
    #[rejection(code = "PARAM_TYPE_MISMATCH")]
    #[error("Could not parse {} as Date: {}", input, source)]
    InvalidDate {
        input: String,
        #[source]
        source: chrono::ParseError,
    },
}

#[derive(Debug, errlanes::Rejection)]
#[rejection(code = "UNSUPPORTED_PARAM_TYPE")]
#[error("Unsupported parameter type: {0:?}")]
pub struct UnsupportedParamType(pub CelType);
