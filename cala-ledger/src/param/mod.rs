pub mod definition;
pub mod error;

use cel_interpreter::{CelContext, CelMap, CelValue};
use es_entity::clock::ClockHandle;
use std::collections::HashMap;
use tracing::instrument;

pub use cala_types::param::*;

use error::*;

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
    pub(crate) fn into_context(
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
