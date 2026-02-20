pub mod definition;
pub mod error;

use cel_interpreter::{CelContext, CelExpression, CelMap, CelType, CelValue};
use es_entity::clock::ClockHandle;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tracing::instrument;

use error::*;

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

#[derive(Clone, Debug)]
pub struct Params {
    values: HashMap<String, CelValue>,
}

impl Params {
    pub fn new() -> Self {
        Self {
            values: HashMap::new(),
        }
    }

    pub fn insert(&mut self, k: impl Into<String>, v: impl Into<CelValue>) {
        self.values.insert(k.into(), v.into());
    }

    #[instrument(name = "params.into_context", skip(self, clock, defs), fields(params_count = self.values.len()), err)]
    pub fn into_context(
        mut self,
        clock: &ClockHandle,
        defs: Option<&Vec<ParamDefinition>>,
    ) -> Result<CelContext, ParamError> {
        let mut ctx = crate::cel_context::initialize(clock.clone());
        if let Some(defs) = defs {
            let mut cel_map = CelMap::new();
            for d in defs {
                if let Some(v) = self.values.remove(&d.name) {
                    cel_map.insert(
                        d.name.clone(),
                        d.r#type
                            .coerce_value(v)
                            .map_err(ParamError::ParamTypeMismatch)?,
                    );
                } else if let Some(expr) = d.default.as_ref() {
                    cel_map.insert(d.name.clone(), expr.evaluate(&ctx)?);
                }
            }
            ctx.add_variable("params", cel_map);
        }

        Ok(ctx)
    }
}

impl Default for Params {
    fn default() -> Self {
        Self::new()
    }
}
