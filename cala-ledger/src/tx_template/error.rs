use super::repo::TxTemplateConstraintViolation;
use crate::param::error::BindParamsRejection;
use cala_types::{cel_error::*, param::*, primitives::*};
use cel_interpreter::*;
use es_entity::errlanes;
use rust_decimal::Decimal;

#[derive(Debug, errlanes::Rejection, errlanes::Lift)]
#[lift(TxTemplateConstraintViolation, unhandled = fatal)]
pub enum CreateTxTemplateRejection {
    #[error("template id already exists: {0}")]
    #[rejection(code = "CALA_TX_TEMPLATE_DUPLICATE_ID")]
    #[lift(TxTemplateConstraintViolation::Pkey, field = attempted)]
    DuplicateId(TxTemplateId),
    #[error("template code already exists: {0:?}")]
    #[rejection(code = "CALA_TX_TEMPLATE_DUPLICATE_CODE")]
    #[lift(TxTemplateConstraintViolation::CodeKey, field = attempted)]
    DuplicateCode(Option<String>),
}

#[derive(Debug, errlanes::Rejection)]
#[rejection(code = "CALA_TX_TEMPLATE_COULD_NOT_FIND_BY_CODE")]
#[error("Template code '{0}' not found")]
pub struct TxTemplateNotFound(pub String);

#[derive(Debug, errlanes::Rejection, errlanes::Lift)]
#[lift(ScalarEvaluationRejection)]
#[lift(ExternalEvaluationRejection)]
#[lift(CurrencyEvaluationRejection)]
#[lift(JsonEvaluationRejection)]
pub enum PrepareEntriesRejection {
    #[rejection(code = "CALA_TX_TEMPLATE_UNBALANCED_TRANSACTION")]
    #[error("Unbalanced transaction: currency {0}, layer {1:?}, amount {2}")]
    UnbalancedTransaction(Currency, Layer, Decimal),
    #[rejection(code = "RESULT_COERCION_ERROR")]
    #[error("{0}")]
    #[rejection(from)]
    #[lift(ScalarEvaluationRejection::CoreTypeCoercion)]
    CoreTypeCoercion(CoreTypeCoercion),
    #[rejection(code = "EVALUATION_ERROR")]
    #[error("Error evaluating expression '{}': {}", expression, source)]
    #[lift(ScalarEvaluationRejection::UnknownIdent)]
    #[lift(ExternalEvaluationRejection::UnknownIdent)]
    #[lift(CurrencyEvaluationRejection::UnknownIdent)]
    #[lift(JsonEvaluationRejection::UnknownIdent)]
    UnknownIdent {
        expression: String,
        #[source]
        source: CelExecutionError,
    },
    #[rejection(code = "EVALUATION_ERROR")]
    #[error("Error evaluating expression '{}': {}", expression, source)]
    #[lift(ScalarEvaluationRejection::MissingArgument)]
    #[lift(ExternalEvaluationRejection::MissingArgument)]
    #[lift(CurrencyEvaluationRejection::MissingArgument)]
    #[lift(JsonEvaluationRejection::MissingArgument)]
    MissingArgument {
        expression: String,
        #[source]
        source: CelExecutionError,
    },
    #[rejection(code = "EVALUATION_ERROR")]
    #[error("Error evaluating expression '{}': {}", expression, source)]
    #[lift(ScalarEvaluationRejection::NoMatchingOverload)]
    #[lift(ExternalEvaluationRejection::NoMatchingOverload)]
    #[lift(CurrencyEvaluationRejection::NoMatchingOverload)]
    #[lift(JsonEvaluationRejection::NoMatchingOverload)]
    NoMatchingOverload {
        expression: String,
        #[source]
        source: CelExecutionError,
    },
    #[rejection(code = "EVALUATION_ERROR")]
    #[error("Error evaluating expression '{}': {}", expression, source)]
    #[lift(ScalarEvaluationRejection::Unexpected)]
    #[lift(ExternalEvaluationRejection::Unexpected)]
    #[lift(CurrencyEvaluationRejection::Unexpected)]
    #[lift(JsonEvaluationRejection::Unexpected)]
    Unexpected {
        expression: String,
        #[source]
        source: CelExecutionError,
    },
    #[rejection(code = "UNEXPECTED")]
    #[error("Unsupported opaque value {} in '{}'", type_name, expression)]
    #[lift(ScalarEvaluationRejection::UnsupportedOpaque)]
    #[lift(ExternalEvaluationRejection::UnsupportedOpaque)]
    #[lift(CurrencyEvaluationRejection::UnsupportedOpaque)]
    #[lift(JsonEvaluationRejection::UnsupportedOpaque)]
    UnsupportedOpaque {
        expression: String,
        type_name: String,
    },
    #[rejection(code = "UNEXPECTED")]
    #[error("Could not downcast {} in '{}'", type_name, expression)]
    #[lift(ScalarEvaluationRejection::OpaqueDowncast)]
    #[lift(ExternalEvaluationRejection::OpaqueDowncast)]
    #[lift(CurrencyEvaluationRejection::OpaqueDowncast)]
    #[lift(JsonEvaluationRejection::OpaqueDowncast)]
    OpaqueDowncast {
        expression: String,
        type_name: &'static str,
    },
    #[rejection(code = "UNEXPECTED")]
    #[error("Cannot convert function value in '{}'", expression)]
    #[lift(ScalarEvaluationRejection::FunctionValue)]
    #[lift(ExternalEvaluationRejection::FunctionValue)]
    #[lift(CurrencyEvaluationRejection::FunctionValue)]
    #[lift(JsonEvaluationRejection::FunctionValue)]
    FunctionValue { expression: String },
    #[rejection(code = "RESULT_COERCION_ERROR")]
    #[error("{0}")]
    #[rejection(from)]
    #[lift(ExternalEvaluationRejection::ExternalTypeCoercion)]
    #[lift(CurrencyEvaluationRejection::ExternalTypeCoercion)]
    ExternalTypeCoercion(ExternalTypeCoercion),
    #[rejection(code = "RESULT_COERCION_ERROR")]
    #[error("Invalid currency in '{}': {}", expression, source)]
    #[lift(CurrencyEvaluationRejection::InvalidCurrency)]
    InvalidCurrency {
        expression: String,
        #[source]
        source: ParseCurrencyError,
    },
    #[rejection(code = "RESULT_COERCION_ERROR")]
    #[error("{0}")]
    #[lift(JsonEvaluationRejection::NonStringKey)]
    NonStringKey(#[source] CoreTypeCoercion),
    #[rejection(code = "RESULT_COERCION_ERROR")]
    #[error("Cannot convert bytes to JSON in '{}'", expression)]
    #[lift(JsonEvaluationRejection::UnsupportedBytes)]
    UnsupportedBytes { expression: String },
}

#[derive(Debug, errlanes::Rejection, errlanes::Lift)]
#[lift(PrepareEntriesRejection)]
#[lift(BindParamsRejection)]
#[lift(ScalarEvaluationRejection)]
#[lift(JsonEvaluationRejection)]
pub enum PrepareTransactionRejection {
    #[rejection(code = "CALA_TX_TEMPLATE_UNBALANCED_TRANSACTION")]
    #[error("Unbalanced transaction: currency {0}, layer {1:?}, amount {2}")]
    #[lift(PrepareEntriesRejection::UnbalancedTransaction)]
    UnbalancedTransaction(Currency, Layer, Decimal),
    #[rejection(code = "RESULT_COERCION_ERROR")]
    #[error("{0}")]
    #[rejection(from)]
    #[lift(PrepareEntriesRejection::CoreTypeCoercion)]
    #[lift(ScalarEvaluationRejection::CoreTypeCoercion)]
    CoreTypeCoercion(CoreTypeCoercion),
    #[rejection(code = "EVALUATION_ERROR")]
    #[error("Error evaluating expression '{}': {}", expression, source)]
    #[lift(PrepareEntriesRejection::UnknownIdent)]
    #[lift(ScalarEvaluationRejection::UnknownIdent)]
    #[lift(JsonEvaluationRejection::UnknownIdent)]
    UnknownIdent {
        expression: String,
        #[source]
        source: CelExecutionError,
    },
    #[rejection(code = "EVALUATION_ERROR")]
    #[error("Error evaluating expression '{}': {}", expression, source)]
    #[lift(PrepareEntriesRejection::MissingArgument)]
    #[lift(ScalarEvaluationRejection::MissingArgument)]
    #[lift(JsonEvaluationRejection::MissingArgument)]
    MissingArgument {
        expression: String,
        #[source]
        source: CelExecutionError,
    },
    #[rejection(code = "EVALUATION_ERROR")]
    #[error("Error evaluating expression '{}': {}", expression, source)]
    #[lift(PrepareEntriesRejection::NoMatchingOverload)]
    #[lift(ScalarEvaluationRejection::NoMatchingOverload)]
    #[lift(JsonEvaluationRejection::NoMatchingOverload)]
    NoMatchingOverload {
        expression: String,
        #[source]
        source: CelExecutionError,
    },
    #[rejection(code = "EVALUATION_ERROR")]
    #[error("Error evaluating expression '{}': {}", expression, source)]
    #[lift(PrepareEntriesRejection::Unexpected)]
    #[lift(ScalarEvaluationRejection::Unexpected)]
    #[lift(JsonEvaluationRejection::Unexpected)]
    Unexpected {
        expression: String,
        #[source]
        source: CelExecutionError,
    },
    #[rejection(code = "UNEXPECTED")]
    #[error("Unsupported opaque value {} in '{}'", type_name, expression)]
    #[lift(PrepareEntriesRejection::UnsupportedOpaque)]
    #[lift(ScalarEvaluationRejection::UnsupportedOpaque)]
    #[lift(JsonEvaluationRejection::UnsupportedOpaque)]
    UnsupportedOpaque {
        expression: String,
        type_name: String,
    },
    #[rejection(code = "UNEXPECTED")]
    #[error("Could not downcast {} in '{}'", type_name, expression)]
    #[lift(PrepareEntriesRejection::OpaqueDowncast)]
    #[lift(ScalarEvaluationRejection::OpaqueDowncast)]
    #[lift(JsonEvaluationRejection::OpaqueDowncast)]
    OpaqueDowncast {
        expression: String,
        type_name: &'static str,
    },
    #[rejection(code = "UNEXPECTED")]
    #[error("Cannot convert function value in '{}'", expression)]
    #[lift(PrepareEntriesRejection::FunctionValue)]
    #[lift(ScalarEvaluationRejection::FunctionValue)]
    #[lift(JsonEvaluationRejection::FunctionValue)]
    FunctionValue { expression: String },
    #[rejection(code = "RESULT_COERCION_ERROR")]
    #[error("{0}")]
    #[rejection(from)]
    #[lift(PrepareEntriesRejection::ExternalTypeCoercion)]
    ExternalTypeCoercion(ExternalTypeCoercion),
    #[rejection(code = "RESULT_COERCION_ERROR")]
    #[error("Invalid currency in '{}': {}", expression, source)]
    #[lift(PrepareEntriesRejection::InvalidCurrency)]
    InvalidCurrency {
        expression: String,
        #[source]
        source: ParseCurrencyError,
    },
    #[rejection(code = "RESULT_COERCION_ERROR")]
    #[error("{0}")]
    #[lift(PrepareEntriesRejection::NonStringKey)]
    #[lift(JsonEvaluationRejection::NonStringKey)]
    NonStringKey(#[source] CoreTypeCoercion),
    #[rejection(code = "RESULT_COERCION_ERROR")]
    #[error("Cannot convert bytes to JSON in '{}'", expression)]
    #[lift(PrepareEntriesRejection::UnsupportedBytes)]
    #[lift(JsonEvaluationRejection::UnsupportedBytes)]
    UnsupportedBytes { expression: String },
    #[rejection(code = "CEL_ERROR")]
    #[error("Error evaluating expression '{}': {}", expression, source)]
    #[lift(BindParamsRejection::DefaultUnknownIdent)]
    DefaultUnknownIdent {
        expression: String,
        #[source]
        source: CelExecutionError,
    },
    #[rejection(code = "CEL_ERROR")]
    #[error("Error evaluating expression '{}': {}", expression, source)]
    #[lift(BindParamsRejection::DefaultMissingArgument)]
    DefaultMissingArgument {
        expression: String,
        #[source]
        source: CelExecutionError,
    },
    #[rejection(code = "CEL_ERROR")]
    #[error("Error evaluating expression '{}': {}", expression, source)]
    #[lift(BindParamsRejection::DefaultNoMatchingOverload)]
    DefaultNoMatchingOverload {
        expression: String,
        #[source]
        source: CelExecutionError,
    },
    #[rejection(code = "CEL_ERROR")]
    #[error("Error evaluating expression '{}': {}", expression, source)]
    #[lift(BindParamsRejection::DefaultUnexpected)]
    DefaultUnexpected {
        expression: String,
        #[source]
        source: CelExecutionError,
    },
    #[rejection(code = "CEL_ERROR")]
    #[error("Unsupported opaque value {} in '{}'", type_name, expression)]
    #[lift(BindParamsRejection::DefaultUnsupportedOpaque)]
    DefaultUnsupportedOpaque {
        expression: String,
        type_name: String,
    },
    #[rejection(code = "CEL_ERROR")]
    #[error("Could not downcast {} in '{}'", type_name, expression)]
    #[lift(BindParamsRejection::DefaultOpaqueDowncast)]
    DefaultOpaqueDowncast {
        expression: String,
        type_name: &'static str,
    },
    #[rejection(code = "CEL_ERROR")]
    #[error("Cannot convert function value in '{}'", expression)]
    #[lift(BindParamsRejection::DefaultFunctionValue)]
    DefaultFunctionValue { expression: String },
    #[rejection(code = "PARAM_TYPE_MISMATCH")]
    #[error("Type mismatch: expected {:?}, got {:?}", expected, actual)]
    #[lift(BindParamsRejection::TypeMismatch)]
    TypeMismatch {
        expected: ParamDataType,
        actual: CelType,
    },
    #[rejection(code = "PARAM_TYPE_MISMATCH")]
    #[error("Could not parse {} as Uuid: {}", input, source)]
    #[lift(BindParamsRejection::InvalidUuid)]
    InvalidUuid {
        input: String,
        #[source]
        source: uuid::Error,
    },
    #[rejection(code = "PARAM_TYPE_MISMATCH")]
    #[error("Could not parse {} as Decimal: {}", input, source)]
    #[lift(BindParamsRejection::InvalidDecimal)]
    InvalidDecimal {
        input: String,
        #[source]
        source: rust_decimal::Error,
    },
    #[rejection(code = "PARAM_TYPE_MISMATCH")]
    #[error("Could not parse {} as Date: {}", input, source)]
    #[lift(BindParamsRejection::InvalidDate)]
    InvalidDate {
        input: String,
        #[source]
        source: chrono::ParseError,
    },
}
