use cala_types::param::*;
use cel_interpreter::*;
use es_entity::errlanes;

#[derive(Debug, errlanes::Rejection, errlanes::Lift)]
#[lift(ParamValueRejection)]
#[lift(CelEvaluationRejection)]
pub enum BindParamsRejection {
    #[rejection(code = "CEL_ERROR")]
    #[error("Error evaluating expression '{}': {}", expression, source)]
    #[lift(CelEvaluationRejection::UnknownIdent)]
    DefaultUnknownIdent {
        expression: String,
        #[source]
        source: CelExecutionError,
    },
    #[rejection(code = "CEL_ERROR")]
    #[error("Error evaluating expression '{}': {}", expression, source)]
    #[lift(CelEvaluationRejection::MissingArgument)]
    DefaultMissingArgument {
        expression: String,
        #[source]
        source: CelExecutionError,
    },
    #[rejection(code = "CEL_ERROR")]
    #[error("Error evaluating expression '{}': {}", expression, source)]
    #[lift(CelEvaluationRejection::NoMatchingOverload)]
    DefaultNoMatchingOverload {
        expression: String,
        #[source]
        source: CelExecutionError,
    },
    #[rejection(code = "CEL_ERROR")]
    #[error("Error evaluating expression '{}': {}", expression, source)]
    #[lift(CelEvaluationRejection::Unexpected)]
    DefaultUnexpected {
        expression: String,
        #[source]
        source: CelExecutionError,
    },
    #[rejection(code = "CEL_ERROR")]
    #[error("Unsupported opaque value {} in '{}'", type_name, expression)]
    #[lift(CelEvaluationRejection::UnsupportedOpaque)]
    DefaultUnsupportedOpaque {
        expression: String,
        type_name: String,
    },
    #[rejection(code = "CEL_ERROR")]
    #[error("Could not downcast {} in '{}'", type_name, expression)]
    #[lift(CelEvaluationRejection::OpaqueDowncast)]
    DefaultOpaqueDowncast {
        expression: String,
        type_name: &'static str,
    },
    #[rejection(code = "CEL_ERROR")]
    #[error("Cannot convert function value in '{}'", expression)]
    #[lift(CelEvaluationRejection::FunctionValue)]
    DefaultFunctionValue { expression: String },
    #[rejection(code = "PARAM_TYPE_MISMATCH")]
    #[error("Type mismatch: expected {:?}, got {:?}", expected, actual)]
    #[lift(ParamValueRejection::TypeMismatch)]
    TypeMismatch {
        expected: ParamDataType,
        actual: CelType,
    },
    #[rejection(code = "PARAM_TYPE_MISMATCH")]
    #[error("Could not parse {} as Uuid: {}", input, source)]
    #[lift(ParamValueRejection::InvalidUuid)]
    InvalidUuid {
        input: String,
        #[source]
        source: uuid::Error,
    },
    #[rejection(code = "PARAM_TYPE_MISMATCH")]
    #[error("Could not parse {} as Decimal: {}", input, source)]
    #[lift(ParamValueRejection::InvalidDecimal)]
    InvalidDecimal {
        input: String,
        #[source]
        source: rust_decimal::Error,
    },
    #[rejection(code = "PARAM_TYPE_MISMATCH")]
    #[error("Could not parse {} as Date: {}", input, source)]
    #[lift(ParamValueRejection::InvalidDate)]
    InvalidDate {
        input: String,
        #[source]
        source: chrono::ParseError,
    },
}
