use crate::cel_type::*;
pub use cel::ExecutionError as CelExecutionError;
use es_entity::errlanes;

/// Hard upper bound on the byte length of an accepted CEL expression.
///
/// The `cel` crate (0.14.x) renders parse errors with a caret line whose
/// width is the error's **column** (`write!(f, "\n| {:.>width$}", "^", ...)`
/// in `parser.rs`), and Rust format widths are `u16`. A parse error past
/// column 65,535 therefore panics with "Formatting argument out of range"
/// inside `ParseError`'s `Display` — and because that panic fires while the
/// parser is already unwinding, it escalates to a process abort (observed
/// as `libFuzzer: deadly signal` in fuzz builds).
///
/// 65,000 leaves margin under the u16 ceiling for EOF-position columns
/// (reported as `len + 1`) while staying far above any legitimate
/// expression: velocity controls and tx-template params are hundreds of
/// bytes.
pub const MAX_EXPRESSION_BYTES: usize = 65_000;

#[derive(Debug, errlanes::Rejection)]
pub enum CelParseRejection {
    #[rejection(code = "CEL_PARSE_ERROR")]
    #[error("Could not parse expression: {0}")]
    ParseError(String),
    #[rejection(code = "EXPRESSION_TOO_LARGE")]
    #[error("Expression is too large: {0} bytes")]
    ExpressionTooLarge(usize),
}

#[derive(Debug, errlanes::Rejection)]
pub enum CelExecutionRejection {
    #[rejection(code = "UNKNOWN_IDENT")]
    #[error("{0}")]
    UnknownIdent(#[source] cel::ExecutionError),
    #[rejection(code = "MISSING_ARGUMENT")]
    #[error("{0}")]
    MissingArgument(#[source] cel::ExecutionError),
    #[rejection(code = "NO_MATCHING_OVERLOAD")]
    #[error("{0}")]
    NoMatchingOverload(#[source] cel::ExecutionError),
    #[rejection(code = "UNEXPECTED")]
    #[error("{0}")]
    Unexpected(#[source] cel::ExecutionError),
}

impl From<cel::ExecutionError> for CelExecutionRejection {
    fn from(error: cel::ExecutionError) -> Self {
        match error {
            e @ (cel::ExecutionError::UndeclaredReference(_)
            | cel::ExecutionError::NoSuchKey(_)) => Self::UnknownIdent(e),
            e @ cel::ExecutionError::NoSuchOverload => Self::NoMatchingOverload(e),
            e @ cel::ExecutionError::InvalidArgumentCount { .. } => Self::MissingArgument(e),
            e => Self::Unexpected(e),
        }
    }
}

#[derive(Debug, errlanes::Rejection)]
pub enum CelValueConversionRejection {
    #[rejection(code = "UNEXPECTED")]
    #[error("Unsupported opaque value {0}")]
    UnsupportedOpaque(String),
    #[rejection(code = "UNEXPECTED")]
    #[error("Could not downcast {0}")]
    OpaqueDowncast(&'static str),
    #[rejection(code = "UNEXPECTED")]
    #[error("Cannot convert function value")]
    FunctionValue,
}

#[derive(Debug, errlanes::Rejection)]
pub enum CelEvaluationRejection {
    #[rejection(code = "EVALUATION_ERROR")]
    #[error("Error evaluating expression '{}': {}", expression, source)]
    UnknownIdent {
        expression: String,
        #[source]
        source: cel::ExecutionError,
    },
    #[rejection(code = "EVALUATION_ERROR")]
    #[error("Error evaluating expression '{}': {}", expression, source)]
    MissingArgument {
        expression: String,
        #[source]
        source: cel::ExecutionError,
    },
    #[rejection(code = "EVALUATION_ERROR")]
    #[error("Error evaluating expression '{}': {}", expression, source)]
    NoMatchingOverload {
        expression: String,
        #[source]
        source: cel::ExecutionError,
    },
    #[rejection(code = "EVALUATION_ERROR")]
    #[error("Error evaluating expression '{}': {}", expression, source)]
    Unexpected {
        expression: String,
        #[source]
        source: cel::ExecutionError,
    },
    #[rejection(code = "UNEXPECTED")]
    #[error("Unsupported opaque value {} in '{}'", type_name, expression)]
    UnsupportedOpaque {
        expression: String,
        type_name: String,
    },
    #[rejection(code = "UNEXPECTED")]
    #[error("Could not downcast {} in '{}'", type_name, expression)]
    OpaqueDowncast {
        expression: String,
        type_name: &'static str,
    },
    #[rejection(code = "UNEXPECTED")]
    #[error("Cannot convert function value in '{}'", expression)]
    FunctionValue { expression: String },
}

impl CelEvaluationRejection {
    pub(crate) fn from_execution(error: CelExecutionRejection, expression: &str) -> Self {
        let expression = expression.to_owned();
        match error {
            CelExecutionRejection::UnknownIdent(source) => {
                Self::UnknownIdent { expression, source }
            }
            CelExecutionRejection::MissingArgument(source) => {
                Self::MissingArgument { expression, source }
            }
            CelExecutionRejection::NoMatchingOverload(source) => {
                Self::NoMatchingOverload { expression, source }
            }
            CelExecutionRejection::Unexpected(source) => Self::Unexpected { expression, source },
        }
    }
    pub(crate) fn from_value(error: CelValueConversionRejection, expression: &str) -> Self {
        let expression = expression.to_owned();
        match error {
            CelValueConversionRejection::UnsupportedOpaque(type_name) => Self::UnsupportedOpaque {
                expression,
                type_name,
            },
            CelValueConversionRejection::OpaqueDowncast(type_name) => Self::OpaqueDowncast {
                expression,
                type_name,
            },
            CelValueConversionRejection::FunctionValue => Self::FunctionValue { expression },
        }
    }
}

#[derive(Debug, errlanes::Rejection)]
#[rejection(code = "BAD_TYPE")]
#[error("Expected {0:?}, found {1:?}")]
pub struct CelTypeMismatch(pub CelType, pub CelType);

#[derive(Debug, errlanes::Rejection)]
#[rejection(code = "BAD_CORE_TYPE_COERCION")]
#[error("Error evaluating expression '{0}' - Could not coerce {1:?} into {2:?}")]
pub struct CoreTypeCoercion(pub String, pub CelType, pub CelType);

#[derive(Debug, errlanes::Rejection)]
#[rejection(code = "BAD_EXTERNAL_TYPE_COERCION")]
#[error("Error evaluating expression '{0}' - Could not coerce {1:?} into {2:?}")]
pub struct ExternalTypeCoercion(pub String, pub CelType, pub &'static str);

#[derive(Debug, errlanes::Rejection, errlanes::Lift)]
#[lift(CelEvaluationRejection)]
pub enum ScalarEvaluationRejection {
    #[rejection(code = "RESULT_COERCION_ERROR")]
    #[error("{0}")]
    #[rejection(from)]
    CoreTypeCoercion(CoreTypeCoercion),
    #[rejection(code = "EVALUATION_ERROR")]
    #[error("Error evaluating expression '{}': {}", expression, source)]
    #[lift(CelEvaluationRejection::UnknownIdent)]
    UnknownIdent {
        expression: String,
        #[source]
        source: cel::ExecutionError,
    },
    #[rejection(code = "EVALUATION_ERROR")]
    #[error("Error evaluating expression '{}': {}", expression, source)]
    #[lift(CelEvaluationRejection::MissingArgument)]
    MissingArgument {
        expression: String,
        #[source]
        source: cel::ExecutionError,
    },
    #[rejection(code = "EVALUATION_ERROR")]
    #[error("Error evaluating expression '{}': {}", expression, source)]
    #[lift(CelEvaluationRejection::NoMatchingOverload)]
    NoMatchingOverload {
        expression: String,
        #[source]
        source: cel::ExecutionError,
    },
    #[rejection(code = "EVALUATION_ERROR")]
    #[error("Error evaluating expression '{}': {}", expression, source)]
    #[lift(CelEvaluationRejection::Unexpected)]
    Unexpected {
        expression: String,
        #[source]
        source: cel::ExecutionError,
    },
    #[rejection(code = "UNEXPECTED")]
    #[error("Unsupported opaque value {} in '{}'", type_name, expression)]
    #[lift(CelEvaluationRejection::UnsupportedOpaque)]
    UnsupportedOpaque {
        expression: String,
        type_name: String,
    },
    #[rejection(code = "UNEXPECTED")]
    #[error("Could not downcast {} in '{}'", type_name, expression)]
    #[lift(CelEvaluationRejection::OpaqueDowncast)]
    OpaqueDowncast {
        expression: String,
        type_name: &'static str,
    },
    #[rejection(code = "UNEXPECTED")]
    #[error("Cannot convert function value in '{}'", expression)]
    #[lift(CelEvaluationRejection::FunctionValue)]
    FunctionValue { expression: String },
}

#[derive(Debug, errlanes::Rejection)]
pub enum JsonCoercionRejection {
    #[rejection(code = "BAD_CORE_TYPE_COERCION")]
    #[error("{0}")]
    #[rejection(from)]
    NonStringKey(#[source] CoreTypeCoercion),
    #[rejection(code = "BAD_EXTERNAL_TYPE_COERCION")]
    #[error("Cannot convert bytes to JSON in '{}'", expression)]
    UnsupportedBytes { expression: String },
}

#[derive(Debug, errlanes::Rejection, errlanes::Lift)]
#[lift(CelEvaluationRejection)]
#[lift(JsonCoercionRejection)]
pub enum JsonEvaluationRejection {
    #[rejection(code = "EVALUATION_ERROR")]
    #[error("Error evaluating expression '{}': {}", expression, source)]
    #[lift(CelEvaluationRejection::UnknownIdent)]
    UnknownIdent {
        expression: String,
        #[source]
        source: cel::ExecutionError,
    },
    #[rejection(code = "EVALUATION_ERROR")]
    #[error("Error evaluating expression '{}': {}", expression, source)]
    #[lift(CelEvaluationRejection::MissingArgument)]
    MissingArgument {
        expression: String,
        #[source]
        source: cel::ExecutionError,
    },
    #[rejection(code = "EVALUATION_ERROR")]
    #[error("Error evaluating expression '{}': {}", expression, source)]
    #[lift(CelEvaluationRejection::NoMatchingOverload)]
    NoMatchingOverload {
        expression: String,
        #[source]
        source: cel::ExecutionError,
    },
    #[rejection(code = "EVALUATION_ERROR")]
    #[error("Error evaluating expression '{}': {}", expression, source)]
    #[lift(CelEvaluationRejection::Unexpected)]
    Unexpected {
        expression: String,
        #[source]
        source: cel::ExecutionError,
    },
    #[rejection(code = "UNEXPECTED")]
    #[error("Unsupported opaque value {} in '{}'", type_name, expression)]
    #[lift(CelEvaluationRejection::UnsupportedOpaque)]
    UnsupportedOpaque {
        expression: String,
        type_name: String,
    },
    #[rejection(code = "UNEXPECTED")]
    #[error("Could not downcast {} in '{}'", type_name, expression)]
    #[lift(CelEvaluationRejection::OpaqueDowncast)]
    OpaqueDowncast {
        expression: String,
        type_name: &'static str,
    },
    #[rejection(code = "UNEXPECTED")]
    #[error("Cannot convert function value in '{}'", expression)]
    #[lift(CelEvaluationRejection::FunctionValue)]
    FunctionValue { expression: String },
    #[rejection(code = "RESULT_COERCION_ERROR")]
    #[error("{0}")]
    #[lift(JsonCoercionRejection::NonStringKey)]
    NonStringKey(#[source] CoreTypeCoercion),
    #[rejection(code = "RESULT_COERCION_ERROR")]
    #[error("Cannot convert bytes to JSON in '{}'", expression)]
    #[lift(JsonCoercionRejection::UnsupportedBytes)]
    UnsupportedBytes { expression: String },
}
