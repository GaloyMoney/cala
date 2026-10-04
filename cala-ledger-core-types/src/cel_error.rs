use super::primitives::ParseCurrencyError;
use cel_interpreter::*;
use es_entity::errlanes;

#[derive(Debug, errlanes::Rejection, errlanes::Lift)]
#[lift(CelEvaluationRejection)]
pub enum ExternalEvaluationRejection {
    #[rejection(code = "RESULT_COERCION_ERROR")]
    #[error("{0}")]
    #[rejection(from)]
    ExternalTypeCoercion(ExternalTypeCoercion),
    #[rejection(code = "EVALUATION_ERROR")]
    #[error("Error evaluating expression '{}': {}", expression, source)]
    #[lift(CelEvaluationRejection::UnknownIdent)]
    UnknownIdent {
        expression: String,
        #[source]
        source: CelExecutionError,
    },
    #[rejection(code = "EVALUATION_ERROR")]
    #[error("Error evaluating expression '{}': {}", expression, source)]
    #[lift(CelEvaluationRejection::MissingArgument)]
    MissingArgument {
        expression: String,
        #[source]
        source: CelExecutionError,
    },
    #[rejection(code = "EVALUATION_ERROR")]
    #[error("Error evaluating expression '{}': {}", expression, source)]
    #[lift(CelEvaluationRejection::NoMatchingOverload)]
    NoMatchingOverload {
        expression: String,
        #[source]
        source: CelExecutionError,
    },
    #[rejection(code = "EVALUATION_ERROR")]
    #[error("Error evaluating expression '{}': {}", expression, source)]
    #[lift(CelEvaluationRejection::Unexpected)]
    Unexpected {
        expression: String,
        #[source]
        source: CelExecutionError,
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
pub enum CurrencyCoercionRejection {
    #[rejection(code = "BAD_EXTERNAL_TYPE_COERCION")]
    #[error("{0}")]
    #[rejection(from)]
    ExternalTypeCoercion(ExternalTypeCoercion),
    #[rejection(code = "EXTERNAL_TYPE_COERCION_ERROR")]
    #[error("Invalid currency in '{}': {}", expression, source)]
    InvalidCurrency {
        expression: String,
        #[source]
        source: ParseCurrencyError,
    },
}

#[derive(Debug, errlanes::Rejection, errlanes::Lift)]
#[lift(CelEvaluationRejection)]
#[lift(CurrencyCoercionRejection)]
pub enum CurrencyEvaluationRejection {
    #[rejection(code = "EVALUATION_ERROR")]
    #[error("Error evaluating expression '{}': {}", expression, source)]
    #[lift(CelEvaluationRejection::UnknownIdent)]
    UnknownIdent {
        expression: String,
        #[source]
        source: CelExecutionError,
    },
    #[rejection(code = "EVALUATION_ERROR")]
    #[error("Error evaluating expression '{}': {}", expression, source)]
    #[lift(CelEvaluationRejection::MissingArgument)]
    MissingArgument {
        expression: String,
        #[source]
        source: CelExecutionError,
    },
    #[rejection(code = "EVALUATION_ERROR")]
    #[error("Error evaluating expression '{}': {}", expression, source)]
    #[lift(CelEvaluationRejection::NoMatchingOverload)]
    NoMatchingOverload {
        expression: String,
        #[source]
        source: CelExecutionError,
    },
    #[rejection(code = "EVALUATION_ERROR")]
    #[error("Error evaluating expression '{}': {}", expression, source)]
    #[lift(CelEvaluationRejection::Unexpected)]
    Unexpected {
        expression: String,
        #[source]
        source: CelExecutionError,
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
    #[lift(CurrencyCoercionRejection::ExternalTypeCoercion)]
    ExternalTypeCoercion(ExternalTypeCoercion),
    #[rejection(code = "RESULT_COERCION_ERROR")]
    #[error("Invalid currency in '{}': {}", expression, source)]
    #[lift(CurrencyCoercionRejection::InvalidCurrency)]
    InvalidCurrency {
        expression: String,
        #[source]
        source: ParseCurrencyError,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives::{Currency, Layer};
    use es_entity::errlanes::Rejection;
    use std::error::Error;

    #[test]
    fn external_targets_have_distinct_coercion_contracts() {
        let context = CelContext::new();
        let expression: CelExpression = "'INVALID'".parse().unwrap();
        let currency: Result<Currency, CurrencyEvaluationRejection> =
            expression.try_evaluate(&context);
        let error = currency.unwrap_err();
        assert_eq!(<&str>::from(error.code()), "RESULT_COERCION_ERROR");
        assert!(error.source().unwrap().is::<ParseCurrencyError>());
        assert!(matches!(
            error,
            CurrencyEvaluationRejection::InvalidCurrency { .. }
        ));
        let layer: Result<Layer, ExternalEvaluationRejection> = expression.try_evaluate(&context);
        assert!(matches!(
            layer,
            Err(ExternalEvaluationRejection::ExternalTypeCoercion(_))
        ));
    }
}
