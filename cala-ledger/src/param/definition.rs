use serde::{Deserialize, Serialize};

pub use cala_types::param::*;
use cel_interpreter::{CelConversionRejection, CelExpression, CelParseRejection};
use es_entity::errlanes;

#[derive(Debug, errlanes::Rejection)]
pub enum InvalidParamDefinition {
    #[error("{0}")]
    #[rejection(delegate, from)]
    Parse(CelParseRejection),
    #[error("{0}")]
    #[rejection(delegate, from)]
    Evaluation(CelConversionRejection),
    #[error("Default expression does not match parameter type {specified:?}: {source}")]
    #[rejection(code = "CALA_PARAM_DEFAULT_TYPE_MISMATCH")]
    DefaultTypeMismatch {
        specified: ParamDataType,
        #[source]
        source: ParamValueRejection,
    },
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct NewParamDefinition {
    pub(crate) name: String,
    pub(crate) r#type: ParamDataType,
    pub(crate) default: Option<String>,
    pub(crate) description: Option<String>,
}

#[bon::bon]
impl NewParamDefinition {
    #[builder]
    pub fn new(
        #[builder(into)] name: String,
        r#type: ParamDataType,
        #[builder(into, name = default_expr)] default: Option<String>,
        #[builder(into)] description: Option<String>,
    ) -> Result<Self, InvalidParamDefinition> {
        use es_entity::clock::Clock;
        if let Some(expr) = default.as_ref() {
            let expr = CelExpression::try_from(expr.as_str())?;
            let value = expr.evaluate(&crate::cel_context::initialize(Clock::handle().clone()))?;
            r#type.coerce_value(value).map_err(|source| {
                InvalidParamDefinition::DefaultTypeMismatch {
                    specified: r#type.clone(),
                    source,
                }
            })?;
        }
        Ok(Self {
            name,
            r#type,
            default,
            description,
        })
    }

    pub fn default_expr(&self) -> Option<CelExpression> {
        self.default
            .as_ref()
            .map(|v| v.parse().expect("Couldn't create default_expr"))
    }
}

impl From<NewParamDefinition> for cala_types::tx_template::ParamDefinition {
    fn from(param: NewParamDefinition) -> Self {
        let default = param.default_expr();
        cala_types::tx_template::ParamDefinition {
            name: param.name,
            r#type: param.r#type,
            default,
            description: param.description,
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_param_definition() {
        let definition = NewParamDefinition::builder()
            .name("name")
            .r#type(ParamDataType::Json)
            .default_expr("{'key': 'value'}")
            .build()
            .unwrap();
        assert_eq!(definition.name, "name");
    }

    #[test]
    fn build_date_param_definition_with_default() {
        let definition = NewParamDefinition::builder()
            .name("effective")
            .r#type(ParamDataType::Date)
            .default_expr("date()")
            .build()
            .unwrap();
        assert_eq!(definition.name, "effective");
    }

    #[test]
    fn rejects_default_not_matching_type() {
        let definition = NewParamDefinition::builder()
            .name("amount")
            .r#type(ParamDataType::Decimal)
            .default_expr("'not a decimal'")
            .build();
        assert!(matches!(
            definition,
            Err(InvalidParamDefinition::DefaultTypeMismatch {
                specified: ParamDataType::Decimal,
                ..
            })
        ));
    }
}
