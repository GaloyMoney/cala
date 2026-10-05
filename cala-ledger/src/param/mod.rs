pub mod definition;

use cel_interpreter::{CelContext, CelConversionRejection, CelMap, CelValue};
use es_entity::clock::ClockHandle;
use std::collections::HashMap;
use tracing::instrument;

pub use cala_types::param::*;

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

    #[instrument(level = "debug", name = "params.into_context", skip(self, clock, defs, reject_value, reject_default), fields(params_count = self.values.len()), err(level = tracing::Level::WARN))]
    pub(crate) fn into_context<R: std::fmt::Display>(
        mut self,
        clock: &ClockHandle,
        defs: Option<&Vec<ParamDefinition>>,
        reject_value: impl Fn(&str, ParamValueRejection) -> R,
        reject_default: impl Fn(&str, CelConversionRejection) -> R,
    ) -> Result<CelContext, R> {
        let mut ctx = crate::cel_context::initialize(clock.clone());
        if let Some(defs) = defs {
            let mut cel_map = CelMap::new();
            for d in defs {
                if let Some(v) = self.values.remove(&d.name) {
                    cel_map.insert(
                        d.name.clone(),
                        d.r#type
                            .coerce_value(v)
                            .map_err(|source| reject_value(&d.name, source))?,
                    );
                } else if let Some(expr) = d.default.as_ref() {
                    cel_map.insert(
                        d.name.clone(),
                        expr.evaluate(&ctx)
                            .map_err(|source| reject_default(&d.name, source))?,
                    );
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        posting::{
            BatchPostingRejection, BatchPreparePostingRejection, PostingRef, PostingRejection,
            PreparePostingRejection,
        },
        primitives::TransactionId,
        velocity::error::AttachVelocityControlRejection,
    };
    use es_entity::{
        clock::Clock,
        errlanes::{Level, Rejection},
    };
    use std::error::Error;

    #[test]
    fn binding_reports_errors_in_the_callers_contract_with_context_and_sources() {
        let posting = PostingRef {
            index: 4,
            tx_id: TransactionId::new(),
        };
        let defs = vec![ParamDefinition {
            name: "account".into(),
            r#type: ParamDataType::Uuid,
            default: Some("missing_variable".parse().unwrap()),
            description: None,
        }];
        for supplied in [false, true] {
            let mut params = Params::new();
            if supplied {
                params.insert("account", "invalid-uuid");
            }
            let for_posting: PostingRejection = params
                .clone()
                .into_context(
                    Clock::handle(),
                    Some(&defs),
                    |parameter, source| PreparePostingRejection::Param {
                        posting,
                        parameter: parameter.to_owned(),
                        source: Box::new(source),
                    },
                    |parameter, source| PreparePostingRejection::Default {
                        posting,
                        parameter: parameter.to_owned(),
                        source: Box::new(source),
                    },
                )
                .unwrap_err()
                .into();
            let for_attachment = params
                .into_context(
                    Clock::handle(),
                    Some(&defs),
                    |_, source| AttachVelocityControlRejection::from(source),
                    |_, source| AttachVelocityControlRejection::from_default(source),
                )
                .unwrap_err();
            assert_eq!(<&str>::from(for_posting.code()), "CALA_POSTING_REJECTED");
            assert_eq!(for_posting.level(), Level::Info);
            assert_eq!(for_posting.to_string(), for_attachment.to_string());
            assert_eq!(
                <&str>::from(for_attachment.code()),
                if supplied {
                    "PARAM_TYPE_MISMATCH"
                } else {
                    "CEL_ERROR"
                }
            );
            if supplied {
                assert!(for_posting
                    .source()
                    .unwrap()
                    .source()
                    .unwrap()
                    .source()
                    .unwrap()
                    .is::<uuid::Error>());
                assert!(for_attachment.source().unwrap().is::<uuid::Error>());
                assert!(
                    matches!(&for_posting, PostingRejection::Prepare(PreparePostingRejection::Param { posting: actual, parameter, source }) if *actual == posting && parameter == "account" && matches!(source.as_ref(), ParamValueRejection::InvalidUuid { input, .. } if input == "invalid-uuid"))
                );
                assert!(matches!(
                    for_attachment,
                    AttachVelocityControlRejection::ParamInvalidUuid { .. }
                ));
            } else {
                assert!(for_posting
                    .source()
                    .unwrap()
                    .source()
                    .unwrap()
                    .source()
                    .unwrap()
                    .is::<cel_interpreter::CelExecutionError>());
                assert!(for_attachment
                    .source()
                    .unwrap()
                    .is::<cel_interpreter::CelExecutionError>());
                assert!(
                    matches!(&for_posting, PostingRejection::Prepare(PreparePostingRejection::Default { posting: actual, parameter, source }) if *actual == posting && parameter == "account" && matches!(source.as_ref(), CelConversionRejection::UnknownIdent { expression, .. } if expression == "missing_variable"))
                );
                assert!(matches!(
                    for_attachment,
                    AttachVelocityControlRejection::DefaultUnknownIdent { .. }
                ));
            }
            let message = for_posting.to_string();
            let batch = BatchPostingRejection::from(for_posting);
            assert_eq!(<&str>::from(batch.code()), "CALA_POSTING_REJECTED");
            assert_eq!(batch.level(), Level::Info);
            assert_eq!(batch.to_string(), message);
            if supplied {
                let source = batch
                    .source()
                    .unwrap()
                    .source()
                    .unwrap()
                    .downcast_ref::<Box<ParamValueRejection>>()
                    .unwrap();
                assert_eq!(<&str>::from(source.code()), "PARAM_TYPE_MISMATCH");
                assert!(source.source().unwrap().is::<uuid::Error>());
                assert!(
                    matches!(batch, BatchPostingRejection::Prepare(BatchPreparePostingRejection::PostingParam {
                    posting: actual, parameter, source
                }) if actual == posting && parameter == "account" && matches!(source.as_ref(), ParamValueRejection::InvalidUuid { input, .. } if input == "invalid-uuid"))
                );
            } else {
                let source = batch
                    .source()
                    .unwrap()
                    .source()
                    .unwrap()
                    .downcast_ref::<Box<CelConversionRejection>>()
                    .unwrap();
                assert_eq!(<&str>::from(source.code()), "CEL_EVALUATION_ERROR");
                assert!(source
                    .source()
                    .unwrap()
                    .is::<cel_interpreter::CelExecutionError>());
                assert!(
                    matches!(batch, BatchPostingRejection::Prepare(BatchPreparePostingRejection::PostingDefault {
                    posting: actual, parameter, source
                }) if actual == posting && parameter == "account" && matches!(source.as_ref(), CelConversionRejection::UnknownIdent { expression, .. } if expression == "missing_variable"))
                );
            }
        }
    }
}
