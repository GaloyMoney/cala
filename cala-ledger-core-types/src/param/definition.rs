use derive_builder::Builder;
use serde::{Deserialize, Serialize};

use super::ParamDataType;
use cel_interpreter::CelExpression;

#[derive(Clone, Debug, Deserialize, Serialize, Builder)]
#[builder(build_fn(validate = "Self::validate"))]
pub struct NewParamDefinition {
    #[builder(setter(into))]
    pub name: String,
    pub r#type: ParamDataType,
    #[builder(setter(strip_option, name = "default_expr", into), default)]
    pub default: Option<String>,
    #[builder(setter(strip_option, into), default)]
    pub description: Option<String>,
}

impl NewParamDefinition {
    pub fn builder() -> NewParamDefinitionBuilder {
        NewParamDefinitionBuilder::default()
    }

    pub fn default_expr(&self) -> Option<CelExpression> {
        self.default
            .as_ref()
            .map(|v| v.parse().expect("Couldn't create default_expr"))
    }
}

impl NewParamDefinitionBuilder {
    fn validate(&self) -> Result<(), String> {
        use es_entity::clock::Clock;
        if let Some(Some(expr)) = self.default.as_ref() {
            let expr = CelExpression::try_from(expr.as_str()).map_err(|e| e.to_string())?;
            let param_type = ParamDataType::try_from(
                &expr
                    .evaluate(&crate::cel_context::initialize(Clock::handle().clone()))
                    .map_err(|e| format!("{e}"))?,
            )?;
            let specified_type = self.r#type.as_ref().unwrap();
            if &param_type != specified_type {
                return Err(format!(
                    "Default expression type {param_type:?} does not match parameter type {specified_type:?}"
                ));
            }
        }
        Ok(())
    }
}

impl From<NewParamDefinition> for super::ParamDefinition {
    fn from(param: NewParamDefinition) -> Self {
        let default = param.default_expr();
        super::ParamDefinition {
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
}
