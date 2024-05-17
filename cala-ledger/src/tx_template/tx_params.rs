use cel_interpreter::{CelContext, CelMap, CelValue};
use std::collections::HashMap;

use super::error::TxTemplateError;
use cala_types::tx_template::ParamDefinition;

#[derive(Debug)]
pub struct TxParams {
    values: HashMap<String, CelValue>,
}

impl TxParams {
    pub fn new() -> Self {
        Self {
            values: HashMap::new(),
        }
    }

    pub fn insert(&mut self, k: impl Into<String>, v: impl Into<CelValue>) {
        self.values.insert(k.into(), v.into());
    }

    pub(crate) fn into_context(
        mut self,
        defs: Option<&Vec<ParamDefinition>>,
    ) -> Result<CelContext, TxTemplateError> {
        let mut ctx = super::cel_context::initialize();
        if let Some(defs) = defs {
            let mut cel_map = CelMap::new();
            for d in defs {
                if let Some(v) = self.values.remove(&d.name) {
                    cel_map.insert(
                        d.name.clone(),
                        d.r#type
                            .coerce_value(v)
                            .map_err(TxTemplateError::TxParamTypeMismatch)?,
                    );
                }
                if let Some(expr) = d.default.as_ref() {
                    cel_map.insert(d.name.clone(), expr.evaluate(&ctx)?);
                }
            }
            ctx.add_variable("params", cel_map);
        }

        if !self.values.is_empty() {
            return Err(TxTemplateError::TooManyParameters);
        }

        Ok(ctx)
    }
}

impl Default for TxParams {
    fn default() -> Self {
        Self::new()
    }
}
