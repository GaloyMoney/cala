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

#[derive(Debug, Clone, errlanes::Rejection)]
pub enum CelParseRejection {
    #[rejection(code = "CEL_PARSE_ERROR")]
    #[error("Could not parse expression: {0}")]
    ParseError(String),
    #[rejection(code = "CEL_EXPRESSION_TOO_LARGE")]
    #[error("Expression is too large: {0} bytes")]
    ExpressionTooLarge(usize),
}

impl CelConversionRejection {
    pub(crate) fn from_execution(source: cel::ExecutionError, expression: &str) -> Self {
        let expression = expression.to_owned();
        match source {
            source @ (cel::ExecutionError::UndeclaredReference(_)
            | cel::ExecutionError::NoSuchKey(_)) => Self::UnknownIdent { expression, source },
            source @ cel::ExecutionError::NoSuchOverload => {
                Self::NoMatchingOverload { expression, source }
            }
            source @ cel::ExecutionError::InvalidArgumentCount { .. } => {
                Self::MissingArgument { expression, source }
            }
            source => Self::Unexpected { expression, source },
        }
    }
}

#[derive(Debug, errlanes::Rejection)]
#[rejection(code = "CEL_BAD_TYPE")]
#[error("Expected {0:?}, found {1:?}")]
pub struct CelTypeMismatch(pub CelType, pub CelType);

#[derive(Debug, errlanes::Rejection)]
#[rejection(code = "CEL_BAD_CORE_TYPE_COERCION")]
#[error("Error evaluating expression '{0}' - Could not coerce {1:?} into {2:?}")]
pub struct CoreTypeCoercion(pub String, pub CelType, pub CelType);

#[derive(Debug, errlanes::Rejection)]
#[rejection(code = "CEL_BAD_EXTERNAL_TYPE_COERCION")]
#[error("Error evaluating expression '{0}' - Could not coerce {1:?} into {2:?}")]
pub struct ExternalTypeCoercion(pub String, pub CelType, pub &'static str);

#[derive(Debug, errlanes::Rejection)]
pub enum JsonCoercionRejection {
    #[error("{0}")]
    #[rejection(code = "CEL_JSON_NON_STRING_KEY", from)]
    NonStringKey(#[source] CoreTypeCoercion),
    #[rejection(code = "CEL_JSON_UNSUPPORTED_BYTES")]
    #[error("Cannot convert bytes to JSON in '{}'", expression)]
    UnsupportedBytes { expression: String },
}

/// Failures from evaluating a CEL expression and converting its result to any target.
///
/// All targets share this contract; individual targets may produce only a subset.
/// External parsers retain their original diagnostic in [`ExternalParseError`].
#[derive(Debug, errlanes::Rejection)]
pub enum CelConversionRejection {
    #[error("{0}")]
    #[rejection(delegate, from)]
    CoreTypeCoercion(CoreTypeCoercion),
    #[rejection(code = "CEL_UNKNOWN_IDENTIFIER")]
    #[error("Error evaluating expression '{}': {}", expression, source)]
    UnknownIdent {
        expression: String,
        #[source]
        source: CelExecutionError,
    },
    #[rejection(code = "CEL_INVALID_ARGUMENT_COUNT")]
    #[error("Error evaluating expression '{}': {}", expression, source)]
    MissingArgument {
        expression: String,
        #[source]
        source: CelExecutionError,
    },
    #[rejection(code = "CEL_NO_MATCHING_OVERLOAD")]
    #[error("Error evaluating expression '{}': {}", expression, source)]
    NoMatchingOverload {
        expression: String,
        #[source]
        source: CelExecutionError,
    },
    #[rejection(code = "CEL_UNEXPECTED_EXECUTION_ERROR")]
    #[error("Error evaluating expression '{}': {}", expression, source)]
    Unexpected {
        expression: String,
        #[source]
        source: CelExecutionError,
    },
    #[rejection(code = "CEL_UNSUPPORTED_OPAQUE_VALUE")]
    #[error("Unsupported opaque value {} in '{}'", type_name, expression)]
    UnsupportedOpaque {
        expression: String,
        type_name: String,
    },
    #[rejection(code = "CEL_OPAQUE_DOWNCAST_FAILED")]
    #[error("Could not downcast {} in '{}'", type_name, expression)]
    OpaqueDowncast {
        expression: String,
        type_name: &'static str,
    },
    #[rejection(code = "CEL_FUNCTION_VALUE")]
    #[error("Cannot convert function value in '{}'", expression)]
    FunctionValue { expression: String },
    #[error("{0}")]
    #[rejection(delegate, from)]
    ExternalTypeCoercion(ExternalTypeCoercion),
    #[error("{0}")]
    #[rejection(delegate, from)]
    Json(JsonCoercionRejection),
    #[error("{0}")]
    #[rejection(delegate, from)]
    ExternalParse(ExternalParseError),
}

/// Diagnostic from a parser for a target defined outside the interpreter crate.
#[derive(Debug, errlanes::Rejection)]
#[rejection(code = "CEL_EXTERNAL_PARSE_ERROR", error = manual)]
pub struct ExternalParseError {
    pub expression: String,
    pub type_name: &'static str,
    pub source: Box<dyn std::error::Error + Send + Sync>,
}

impl std::fmt::Display for ExternalParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Invalid {} in '{}': {}",
            self.type_name, self.expression, self.source
        )
    }
}

impl std::error::Error for ExternalParseError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self.source.as_ref())
    }
}
