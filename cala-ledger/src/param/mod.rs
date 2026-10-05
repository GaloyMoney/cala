pub mod definition;

use cel_interpreter::{CelContext, CelConversionRejection, CelMap, CelValue};
use es_entity::clock::ClockHandle;
use es_entity::errlanes;
use std::collections::HashMap;
use tracing::instrument;

pub use cala_types::param::*;

/// Evaluating an omitted parameter's default and coercing it to its declared type.
#[derive(Debug, errlanes::Rejection)]
pub enum ParamDefaultRejection {
    #[error("{0}")]
    #[rejection(delegate, from)]
    Evaluation(CelConversionRejection),
    #[error("{0}")]
    #[rejection(delegate, from)]
    Value(ParamValueRejection),
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

    #[instrument(level = "debug", name = "params.into_context", skip(self, clock, defs, reject_value, reject_default), fields(params_count = self.values.len()), err(level = tracing::Level::WARN))]
    pub(crate) fn into_context<R: std::fmt::Display>(
        mut self,
        clock: &ClockHandle,
        defs: Option<&Vec<ParamDefinition>>,
        reject_value: impl Fn(&str, ParamValueRejection) -> R,
        reject_default: impl Fn(&str, ParamDefaultRejection) -> R,
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
                    let value = expr
                        .evaluate(&ctx)
                        .map_err(|source| reject_default(&d.name, source.into()))?;
                    cel_map.insert(
                        d.name.clone(),
                        d.r#type
                            .coerce_value(value)
                            .map_err(|source| reject_default(&d.name, source.into()))?,
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
    use cel_interpreter::{CelExpression, CelType};
    use chrono::{TimeZone, Utc};
    use es_entity::{
        clock::Clock,
        errlanes::{Level, Rejection},
    };
    use std::error::Error;

    fn definition(r#type: ParamDataType, default: &str) -> Vec<ParamDefinition> {
        vec![ParamDefinition {
            name: "value".into(),
            r#type,
            default: Some(default.parse().unwrap()),
            description: None,
        }]
    }

    fn bind(
        params: Params,
        defs: &Vec<ParamDefinition>,
        clock: &ClockHandle,
    ) -> Result<CelContext, AttachVelocityControlRejection> {
        params.into_context(
            clock,
            Some(defs),
            |_, source| source.into(),
            |_, source| AttachVelocityControlRejection::Default(source),
        )
    }

    fn has_source<T: Error + 'static>(error: &dyn Error) -> bool {
        let mut source = error.source();
        while let Some(error) = source {
            if error.is::<T>() {
                return true;
            }
            source = error.source();
        }
        false
    }

    #[test]
    fn defaults_and_supplied_values_have_the_same_declared_representation() {
        let now = Utc.with_ymd_and_hms(2025, 6, 15, 10, 30, 0).unwrap();
        let (clock, _control) = ClockHandle::manual_at(now);
        let id = uuid::Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap();
        let decimal = rust_decimal::Decimal::new(125, 2);
        let cases = [
            (
                ParamDataType::Uuid,
                "'00000000-0000-0000-0000-000000000001'",
                CelValue::from(id.to_string()),
                CelValue::Uuid(id),
            ),
            (
                ParamDataType::Decimal,
                "'1.25'",
                CelValue::from("1.25"),
                CelValue::Decimal(decimal),
            ),
            (
                ParamDataType::Date,
                "'2025-06-15'",
                CelValue::from("2025-06-15"),
                CelValue::Date(now.date_naive()),
            ),
            (
                ParamDataType::Date,
                "date()",
                CelValue::Timestamp(now),
                CelValue::Date(now.date_naive()),
            ),
            (
                ParamDataType::Timestamp,
                "timestamp('2025-06-15T10:30:00Z')",
                CelValue::Timestamp(now),
                CelValue::Timestamp(now),
            ),
            (
                ParamDataType::Boolean,
                "true",
                CelValue::Bool(true),
                CelValue::Bool(true),
            ),
            (
                ParamDataType::Integer,
                "42",
                CelValue::Int(42),
                CelValue::Int(42),
            ),
        ];
        for (ty, default, supplied, canonical) in cases {
            let defs = definition(ty, default);
            let defaulted = bind(Params::new(), &defs, &clock).unwrap();
            let mut params = Params::new();
            params.insert("value", supplied);
            let supplied = bind(params, &defs, &clock).unwrap();
            let mut expected = crate::cel_context::initialize(clock.clone());
            let mut values = CelMap::new();
            values.insert("value", canonical);
            expected.add_variable("params", values);
            // Date values become CEL timestamps on evaluation. Compare the
            // stored diagnostic context as well to catch an uncoerced date().
            assert_eq!(
                defaulted.debug_context(),
                expected.debug_context(),
                "{default}"
            );
            assert_eq!(
                supplied.debug_context(),
                expected.debug_context(),
                "{default}"
            );
            let expression: CelExpression = "params.value".parse().unwrap();
            assert_eq!(
                expression.evaluate(&defaulted).unwrap(),
                expression.evaluate(&supplied).unwrap(),
                "{default}"
            );
        }
    }

    #[test]
    fn invalid_defaults_keep_default_context_and_typed_causes_through_batch_posting() {
        let posting = PostingRef {
            index: 4,
            tx_id: TransactionId::new(),
        };
        for (ty, default, cause_code) in [
            (ParamDataType::Uuid, "'invalid-uuid'", "PARAM_INVALID_UUID"),
            (
                ParamDataType::Decimal,
                "'invalid-decimal'",
                "PARAM_INVALID_DECIMAL",
            ),
            (ParamDataType::Date, "'invalid-date'", "PARAM_INVALID_DATE"),
            (ParamDataType::Boolean, "42", "PARAM_TYPE_MISMATCH"),
            (
                ParamDataType::Uuid,
                "missing_variable",
                "CEL_UNKNOWN_IDENTIFIER",
            ),
        ] {
            let defs = definition(ty, default);
            let attachment = bind(Params::new(), &defs, Clock::handle()).unwrap_err();
            assert_eq!(
                <&str>::from(attachment.code()),
                "CALA_VELOCITY_PARAMETER_DEFAULT_FAILED"
            );
            let source = attachment
                .source()
                .unwrap()
                .downcast_ref::<ParamDefaultRejection>()
                .unwrap();
            assert_eq!(<&str>::from(source.code()), cause_code);
            match source {
                ParamDefaultRejection::Value(ParamValueRejection::InvalidUuid {
                    input, ..
                }) => {
                    assert_eq!(input, "invalid-uuid");
                    assert!(has_source::<uuid::Error>(&attachment));
                }
                ParamDefaultRejection::Value(ParamValueRejection::InvalidDecimal { .. }) => {
                    assert!(has_source::<rust_decimal::Error>(&attachment))
                }
                ParamDefaultRejection::Value(ParamValueRejection::InvalidDate { .. }) => {
                    assert!(has_source::<chrono::ParseError>(&attachment))
                }
                ParamDefaultRejection::Value(ParamValueRejection::TypeMismatch {
                    expected,
                    actual,
                }) => {
                    assert_eq!(*expected, ParamDataType::Boolean);
                    assert_eq!(*actual, CelType::Int);
                }
                ParamDefaultRejection::Evaluation(CelConversionRejection::UnknownIdent {
                    ..
                }) => assert!(has_source::<cel_interpreter::CelExecutionError>(
                    &attachment
                )),
                other => panic!("unexpected cause: {other:?}"),
            }
            let single: PostingRejection = Params::new()
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
            assert_eq!(
                <&str>::from(single.code()),
                "CALA_POSTING_PARAMETER_DEFAULT_FAILED"
            );
            assert_eq!(single.level(), Level::Info);
            assert_eq!(single.to_string(), attachment.to_string());
            let batch = BatchPostingRejection::from(single);
            assert_eq!(
                <&str>::from(batch.code()),
                "CALA_POSTING_PARAMETER_DEFAULT_FAILED"
            );
            let BatchPostingRejection::Prepare(BatchPreparePostingRejection::Default {
                posting: actual,
                parameter,
                source,
            }) = batch
            else {
                panic!("default rejection")
            };
            assert_eq!(actual, posting);
            assert_eq!(parameter, "value");
            assert_eq!(<&str>::from(source.code()), cause_code);
            assert_eq!(source.to_string(), attachment.to_string());
        }
    }

    #[test]
    fn supplied_values_override_defaults_and_keep_the_supplied_value_error_contract() {
        let defs = definition(ParamDataType::Uuid, "missing_variable");
        let id = uuid::Uuid::now_v7();
        let mut valid = Params::new();
        valid.insert("value", id);
        assert!(bind(valid, &defs, Clock::handle()).is_ok());
        let mut invalid = Params::new();
        invalid.insert("value", "invalid-uuid");
        let attachment = bind(invalid.clone(), &defs, Clock::handle()).unwrap_err();
        assert_eq!(
            <&str>::from(attachment.code()),
            "CALA_VELOCITY_PARAMETER_INVALID"
        );
        assert!(matches!(
            attachment,
            AttachVelocityControlRejection::Param(ParamValueRejection::InvalidUuid { .. })
        ));
        assert!(has_source::<uuid::Error>(&attachment));
        let posting = PostingRef {
            index: 1,
            tx_id: TransactionId::new(),
        };
        let rejection = invalid
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
            .unwrap_err();
        let batch = BatchPostingRejection::from(rejection);
        assert_eq!(<&str>::from(batch.code()), "CALA_POSTING_PARAMETER_INVALID");
        assert!(has_source::<uuid::Error>(&batch));
        assert!(
            matches!(batch, BatchPostingRejection::Prepare(BatchPreparePostingRejection::Param { posting: actual, parameter, source }) if actual == posting && parameter == "value" && matches!(source.as_ref(), ParamValueRejection::InvalidUuid { .. }))
        );
    }
}
