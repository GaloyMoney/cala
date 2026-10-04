use super::{control::VelocityControlConstraintViolation, limit::VelocityLimitConstraintViolation};
use crate::{param::error::BindParamsRejection, primitives::*};
use cala_types::{cel_error::*, param::*};
use cel_interpreter::*;
use es_entity::errlanes;
use rust_decimal::Decimal;

#[derive(errlanes::Rejection, Debug)]
#[rejection(code = "CALA_VELOCITY_LIMIT_EXCEEDED")]
#[error("Velocity limit exceeded")]
pub struct LimitExceededError {
    pub account_id: AccountId,
    pub currency: Currency,
    pub limit_id: VelocityLimitId,
    pub layer: Layer,
    pub direction: DebitOrCredit,
    pub limit: Decimal,
    pub requested: Decimal,
}

#[derive(Debug, errlanes::Rejection, errlanes::Lift)]
#[lift(VelocityControlConstraintViolation, unhandled = fatal)]
pub enum CreateVelocityControlRejection {
    #[error("Velocity control id already exists: {0}")]
    #[rejection(code = "CALA_VELOCITY_CONTROL_ID_ALREADY_EXISTS")]
    #[lift(VelocityControlConstraintViolation::Pkey, field = attempted)]
    ControlIdAlreadyExists(VelocityControlId),
}

#[derive(Debug, errlanes::Rejection, errlanes::Lift)]
#[lift(VelocityLimitConstraintViolation, unhandled = fatal)]
pub enum CreateVelocityLimitRejection {
    #[error("Velocity limit id already exists: {0}")]
    #[rejection(code = "CALA_VELOCITY_LIMIT_ID_ALREADY_EXISTS")]
    #[lift(VelocityLimitConstraintViolation::Pkey, field = attempted)]
    LimitIdAlreadyExists(VelocityLimitId),
}

#[derive(Debug, errlanes::Rejection)]
#[rejection(code = "CALA_VELOCITY_LIMIT_ALREADY_ADDED_TO_CONTROL")]
#[error("Limit already added to control")]
pub struct LimitAlreadyAddedToControl;

#[derive(Debug, errlanes::Rejection, errlanes::Lift)]
#[lift(BindParamsRejection)]
#[lift(ScalarEvaluationRejection)]
#[lift(ExternalEvaluationRejection)]
pub enum EvaluateVelocityLimitsRejection {
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
    #[rejection(code = "RESULT_COERCION_ERROR")]
    #[error("{0}")]
    #[rejection(from)]
    #[lift(ScalarEvaluationRejection::CoreTypeCoercion)]
    CoreTypeCoercion(CoreTypeCoercion),
    #[rejection(code = "EVALUATION_ERROR")]
    #[error("Error evaluating expression '{}': {}", expression, source)]
    #[lift(ScalarEvaluationRejection::UnknownIdent)]
    #[lift(ExternalEvaluationRejection::UnknownIdent)]
    UnknownIdent {
        expression: String,
        #[source]
        source: CelExecutionError,
    },
    #[rejection(code = "EVALUATION_ERROR")]
    #[error("Error evaluating expression '{}': {}", expression, source)]
    #[lift(ScalarEvaluationRejection::MissingArgument)]
    #[lift(ExternalEvaluationRejection::MissingArgument)]
    MissingArgument {
        expression: String,
        #[source]
        source: CelExecutionError,
    },
    #[rejection(code = "EVALUATION_ERROR")]
    #[error("Error evaluating expression '{}': {}", expression, source)]
    #[lift(ScalarEvaluationRejection::NoMatchingOverload)]
    #[lift(ExternalEvaluationRejection::NoMatchingOverload)]
    NoMatchingOverload {
        expression: String,
        #[source]
        source: CelExecutionError,
    },
    #[rejection(code = "EVALUATION_ERROR")]
    #[error("Error evaluating expression '{}': {}", expression, source)]
    #[lift(ScalarEvaluationRejection::Unexpected)]
    #[lift(ExternalEvaluationRejection::Unexpected)]
    Unexpected {
        expression: String,
        #[source]
        source: CelExecutionError,
    },
    #[rejection(code = "UNEXPECTED")]
    #[error("Unsupported opaque value {} in '{}'", type_name, expression)]
    #[lift(ScalarEvaluationRejection::UnsupportedOpaque)]
    #[lift(ExternalEvaluationRejection::UnsupportedOpaque)]
    UnsupportedOpaque {
        expression: String,
        type_name: String,
    },
    #[rejection(code = "UNEXPECTED")]
    #[error("Could not downcast {} in '{}'", type_name, expression)]
    #[lift(ScalarEvaluationRejection::OpaqueDowncast)]
    #[lift(ExternalEvaluationRejection::OpaqueDowncast)]
    OpaqueDowncast {
        expression: String,
        type_name: &'static str,
    },
    #[rejection(code = "UNEXPECTED")]
    #[error("Cannot convert function value in '{}'", expression)]
    #[lift(ScalarEvaluationRejection::FunctionValue)]
    #[lift(ExternalEvaluationRejection::FunctionValue)]
    FunctionValue { expression: String },
    #[rejection(code = "RESULT_COERCION_ERROR")]
    #[error("{0}")]
    #[rejection(from)]
    #[lift(ExternalEvaluationRejection::ExternalTypeCoercion)]
    ExternalTypeCoercion(ExternalTypeCoercion),
}

#[derive(Debug, errlanes::Rejection, errlanes::Lift)]
#[lift(EvaluateVelocityLimitsRejection)]
pub enum AttachVelocityControlRejection {
    #[rejection(code = "CALA_VELOCITY_COULD_NOT_FIND_CONTROL_BY_ID")]
    #[error("Velocity control not found: {0}")]
    ControlNotFound(VelocityControlId),
    #[rejection(code = "CEL_ERROR")]
    #[error("Error evaluating expression '{}': {}", expression, source)]
    #[lift(EvaluateVelocityLimitsRejection::DefaultUnknownIdent)]
    DefaultUnknownIdent {
        expression: String,
        #[source]
        source: CelExecutionError,
    },
    #[rejection(code = "CEL_ERROR")]
    #[error("Error evaluating expression '{}': {}", expression, source)]
    #[lift(EvaluateVelocityLimitsRejection::DefaultMissingArgument)]
    DefaultMissingArgument {
        expression: String,
        #[source]
        source: CelExecutionError,
    },
    #[rejection(code = "CEL_ERROR")]
    #[error("Error evaluating expression '{}': {}", expression, source)]
    #[lift(EvaluateVelocityLimitsRejection::DefaultNoMatchingOverload)]
    DefaultNoMatchingOverload {
        expression: String,
        #[source]
        source: CelExecutionError,
    },
    #[rejection(code = "CEL_ERROR")]
    #[error("Error evaluating expression '{}': {}", expression, source)]
    #[lift(EvaluateVelocityLimitsRejection::DefaultUnexpected)]
    DefaultUnexpected {
        expression: String,
        #[source]
        source: CelExecutionError,
    },
    #[rejection(code = "CEL_ERROR")]
    #[error("Unsupported opaque value {} in '{}'", type_name, expression)]
    #[lift(EvaluateVelocityLimitsRejection::DefaultUnsupportedOpaque)]
    DefaultUnsupportedOpaque {
        expression: String,
        type_name: String,
    },
    #[rejection(code = "CEL_ERROR")]
    #[error("Could not downcast {} in '{}'", type_name, expression)]
    #[lift(EvaluateVelocityLimitsRejection::DefaultOpaqueDowncast)]
    DefaultOpaqueDowncast {
        expression: String,
        type_name: &'static str,
    },
    #[rejection(code = "CEL_ERROR")]
    #[error("Cannot convert function value in '{}'", expression)]
    #[lift(EvaluateVelocityLimitsRejection::DefaultFunctionValue)]
    DefaultFunctionValue { expression: String },
    #[rejection(code = "PARAM_TYPE_MISMATCH")]
    #[error("Type mismatch: expected {:?}, got {:?}", expected, actual)]
    #[lift(EvaluateVelocityLimitsRejection::TypeMismatch)]
    TypeMismatch {
        expected: ParamDataType,
        actual: CelType,
    },
    #[rejection(code = "PARAM_TYPE_MISMATCH")]
    #[error("Could not parse {} as Uuid: {}", input, source)]
    #[lift(EvaluateVelocityLimitsRejection::InvalidUuid)]
    InvalidUuid {
        input: String,
        #[source]
        source: uuid::Error,
    },
    #[rejection(code = "PARAM_TYPE_MISMATCH")]
    #[error("Could not parse {} as Decimal: {}", input, source)]
    #[lift(EvaluateVelocityLimitsRejection::InvalidDecimal)]
    InvalidDecimal {
        input: String,
        #[source]
        source: rust_decimal::Error,
    },
    #[rejection(code = "PARAM_TYPE_MISMATCH")]
    #[error("Could not parse {} as Date: {}", input, source)]
    #[lift(EvaluateVelocityLimitsRejection::InvalidDate)]
    InvalidDate {
        input: String,
        #[source]
        source: chrono::ParseError,
    },
    #[rejection(code = "RESULT_COERCION_ERROR")]
    #[error("{0}")]
    #[rejection(from)]
    #[lift(EvaluateVelocityLimitsRejection::CoreTypeCoercion)]
    CoreTypeCoercion(CoreTypeCoercion),
    #[rejection(code = "EVALUATION_ERROR")]
    #[error("Error evaluating expression '{}': {}", expression, source)]
    #[lift(EvaluateVelocityLimitsRejection::UnknownIdent)]
    UnknownIdent {
        expression: String,
        #[source]
        source: CelExecutionError,
    },
    #[rejection(code = "EVALUATION_ERROR")]
    #[error("Error evaluating expression '{}': {}", expression, source)]
    #[lift(EvaluateVelocityLimitsRejection::MissingArgument)]
    MissingArgument {
        expression: String,
        #[source]
        source: CelExecutionError,
    },
    #[rejection(code = "EVALUATION_ERROR")]
    #[error("Error evaluating expression '{}': {}", expression, source)]
    #[lift(EvaluateVelocityLimitsRejection::NoMatchingOverload)]
    NoMatchingOverload {
        expression: String,
        #[source]
        source: CelExecutionError,
    },
    #[rejection(code = "EVALUATION_ERROR")]
    #[error("Error evaluating expression '{}': {}", expression, source)]
    #[lift(EvaluateVelocityLimitsRejection::Unexpected)]
    Unexpected {
        expression: String,
        #[source]
        source: CelExecutionError,
    },
    #[rejection(code = "UNEXPECTED")]
    #[error("Unsupported opaque value {} in '{}'", type_name, expression)]
    #[lift(EvaluateVelocityLimitsRejection::UnsupportedOpaque)]
    UnsupportedOpaque {
        expression: String,
        type_name: String,
    },
    #[rejection(code = "UNEXPECTED")]
    #[error("Could not downcast {} in '{}'", type_name, expression)]
    #[lift(EvaluateVelocityLimitsRejection::OpaqueDowncast)]
    OpaqueDowncast {
        expression: String,
        type_name: &'static str,
    },
    #[rejection(code = "UNEXPECTED")]
    #[error("Cannot convert function value in '{}'", expression)]
    #[lift(EvaluateVelocityLimitsRejection::FunctionValue)]
    FunctionValue { expression: String },
    #[rejection(code = "RESULT_COERCION_ERROR")]
    #[error("{0}")]
    #[rejection(from)]
    #[lift(EvaluateVelocityLimitsRejection::ExternalTypeCoercion)]
    ExternalTypeCoercion(ExternalTypeCoercion),
}

#[derive(Debug, errlanes::Rejection, errlanes::Lift)]
#[lift(ScalarEvaluationRejection)]
#[lift(JsonEvaluationRejection)]
pub enum VelocityWindowRejection {
    #[rejection(code = "RESULT_COERCION_ERROR")]
    #[error("{0}")]
    #[rejection(from)]
    #[lift(ScalarEvaluationRejection::CoreTypeCoercion)]
    CoreTypeCoercion(CoreTypeCoercion),
    #[rejection(code = "EVALUATION_ERROR")]
    #[error("Error evaluating expression '{}': {}", expression, source)]
    #[lift(ScalarEvaluationRejection::UnknownIdent)]
    #[lift(JsonEvaluationRejection::UnknownIdent)]
    UnknownIdent {
        expression: String,
        #[source]
        source: CelExecutionError,
    },
    #[rejection(code = "EVALUATION_ERROR")]
    #[error("Error evaluating expression '{}': {}", expression, source)]
    #[lift(ScalarEvaluationRejection::MissingArgument)]
    #[lift(JsonEvaluationRejection::MissingArgument)]
    MissingArgument {
        expression: String,
        #[source]
        source: CelExecutionError,
    },
    #[rejection(code = "EVALUATION_ERROR")]
    #[error("Error evaluating expression '{}': {}", expression, source)]
    #[lift(ScalarEvaluationRejection::NoMatchingOverload)]
    #[lift(JsonEvaluationRejection::NoMatchingOverload)]
    NoMatchingOverload {
        expression: String,
        #[source]
        source: CelExecutionError,
    },
    #[rejection(code = "EVALUATION_ERROR")]
    #[error("Error evaluating expression '{}': {}", expression, source)]
    #[lift(ScalarEvaluationRejection::Unexpected)]
    #[lift(JsonEvaluationRejection::Unexpected)]
    Unexpected {
        expression: String,
        #[source]
        source: CelExecutionError,
    },
    #[rejection(code = "UNEXPECTED")]
    #[error("Unsupported opaque value {} in '{}'", type_name, expression)]
    #[lift(ScalarEvaluationRejection::UnsupportedOpaque)]
    #[lift(JsonEvaluationRejection::UnsupportedOpaque)]
    UnsupportedOpaque {
        expression: String,
        type_name: String,
    },
    #[rejection(code = "UNEXPECTED")]
    #[error("Could not downcast {} in '{}'", type_name, expression)]
    #[lift(ScalarEvaluationRejection::OpaqueDowncast)]
    #[lift(JsonEvaluationRejection::OpaqueDowncast)]
    OpaqueDowncast {
        expression: String,
        type_name: &'static str,
    },
    #[rejection(code = "UNEXPECTED")]
    #[error("Cannot convert function value in '{}'", expression)]
    #[lift(ScalarEvaluationRejection::FunctionValue)]
    #[lift(JsonEvaluationRejection::FunctionValue)]
    FunctionValue { expression: String },
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
#[lift(ScalarEvaluationRejection)]
pub enum EnforceVelocityRejection {
    #[rejection(code = "CALA_VELOCITY_LIMIT_EXCEEDED")]
    #[error("{0}")]
    #[rejection(from)]
    LimitExceeded(LimitExceededError),
    #[rejection(code = "RESULT_COERCION_ERROR")]
    #[error("{0}")]
    #[rejection(from)]
    #[lift(ScalarEvaluationRejection::CoreTypeCoercion)]
    CoreTypeCoercion(CoreTypeCoercion),
    #[rejection(code = "EVALUATION_ERROR")]
    #[error("Error evaluating expression '{}': {}", expression, source)]
    #[lift(ScalarEvaluationRejection::UnknownIdent)]
    UnknownIdent {
        expression: String,
        #[source]
        source: CelExecutionError,
    },
    #[rejection(code = "EVALUATION_ERROR")]
    #[error("Error evaluating expression '{}': {}", expression, source)]
    #[lift(ScalarEvaluationRejection::MissingArgument)]
    MissingArgument {
        expression: String,
        #[source]
        source: CelExecutionError,
    },
    #[rejection(code = "EVALUATION_ERROR")]
    #[error("Error evaluating expression '{}': {}", expression, source)]
    #[lift(ScalarEvaluationRejection::NoMatchingOverload)]
    NoMatchingOverload {
        expression: String,
        #[source]
        source: CelExecutionError,
    },
    #[rejection(code = "EVALUATION_ERROR")]
    #[error("Error evaluating expression '{}': {}", expression, source)]
    #[lift(ScalarEvaluationRejection::Unexpected)]
    Unexpected {
        expression: String,
        #[source]
        source: CelExecutionError,
    },
    #[rejection(code = "UNEXPECTED")]
    #[error("Unsupported opaque value {} in '{}'", type_name, expression)]
    #[lift(ScalarEvaluationRejection::UnsupportedOpaque)]
    UnsupportedOpaque {
        expression: String,
        type_name: String,
    },
    #[rejection(code = "UNEXPECTED")]
    #[error("Could not downcast {} in '{}'", type_name, expression)]
    #[lift(ScalarEvaluationRejection::OpaqueDowncast)]
    OpaqueDowncast {
        expression: String,
        type_name: &'static str,
    },
    #[rejection(code = "UNEXPECTED")]
    #[error("Cannot convert function value in '{}'", expression)]
    #[lift(ScalarEvaluationRejection::FunctionValue)]
    FunctionValue { expression: String },
}

#[derive(Debug, errlanes::Rejection, errlanes::Lift)]
#[lift(VelocityWindowRejection)]
#[lift(EnforceVelocityRejection)]
pub enum EnforceVelocityBatchRejection {
    #[rejection(code = "RESULT_COERCION_ERROR")]
    #[error("{0}")]
    #[rejection(from)]
    #[lift(VelocityWindowRejection::CoreTypeCoercion)]
    #[lift(EnforceVelocityRejection::CoreTypeCoercion)]
    CoreTypeCoercion(CoreTypeCoercion),
    #[rejection(code = "EVALUATION_ERROR")]
    #[error("Error evaluating expression '{}': {}", expression, source)]
    #[lift(VelocityWindowRejection::UnknownIdent)]
    #[lift(EnforceVelocityRejection::UnknownIdent)]
    UnknownIdent {
        expression: String,
        #[source]
        source: CelExecutionError,
    },
    #[rejection(code = "EVALUATION_ERROR")]
    #[error("Error evaluating expression '{}': {}", expression, source)]
    #[lift(VelocityWindowRejection::MissingArgument)]
    #[lift(EnforceVelocityRejection::MissingArgument)]
    MissingArgument {
        expression: String,
        #[source]
        source: CelExecutionError,
    },
    #[rejection(code = "EVALUATION_ERROR")]
    #[error("Error evaluating expression '{}': {}", expression, source)]
    #[lift(VelocityWindowRejection::NoMatchingOverload)]
    #[lift(EnforceVelocityRejection::NoMatchingOverload)]
    NoMatchingOverload {
        expression: String,
        #[source]
        source: CelExecutionError,
    },
    #[rejection(code = "EVALUATION_ERROR")]
    #[error("Error evaluating expression '{}': {}", expression, source)]
    #[lift(VelocityWindowRejection::Unexpected)]
    #[lift(EnforceVelocityRejection::Unexpected)]
    Unexpected {
        expression: String,
        #[source]
        source: CelExecutionError,
    },
    #[rejection(code = "UNEXPECTED")]
    #[error("Unsupported opaque value {} in '{}'", type_name, expression)]
    #[lift(VelocityWindowRejection::UnsupportedOpaque)]
    #[lift(EnforceVelocityRejection::UnsupportedOpaque)]
    UnsupportedOpaque {
        expression: String,
        type_name: String,
    },
    #[rejection(code = "UNEXPECTED")]
    #[error("Could not downcast {} in '{}'", type_name, expression)]
    #[lift(VelocityWindowRejection::OpaqueDowncast)]
    #[lift(EnforceVelocityRejection::OpaqueDowncast)]
    OpaqueDowncast {
        expression: String,
        type_name: &'static str,
    },
    #[rejection(code = "UNEXPECTED")]
    #[error("Cannot convert function value in '{}'", expression)]
    #[lift(VelocityWindowRejection::FunctionValue)]
    #[lift(EnforceVelocityRejection::FunctionValue)]
    FunctionValue { expression: String },
    #[rejection(code = "RESULT_COERCION_ERROR")]
    #[error("{0}")]
    #[lift(VelocityWindowRejection::NonStringKey)]
    NonStringKey(#[source] CoreTypeCoercion),
    #[rejection(code = "RESULT_COERCION_ERROR")]
    #[error("Cannot convert bytes to JSON in '{}'", expression)]
    #[lift(VelocityWindowRejection::UnsupportedBytes)]
    UnsupportedBytes { expression: String },
    #[rejection(code = "CALA_VELOCITY_LIMIT_EXCEEDED")]
    #[error("{0}")]
    #[rejection(from)]
    #[lift(EnforceVelocityRejection::LimitExceeded)]
    LimitExceeded(LimitExceededError),
}

/// Classifies this write's known constraints; every other SQL failure keeps its native lane.
#[derive(Debug, errlanes::Classify)]
pub(crate) enum AttachLimit {
    #[classify(delegate)]
    Domain(LimitAlreadyAddedToControl),
    #[classify(delegate)]
    Sqlx(sqlx::Error),
}
impl From<sqlx::Error> for AttachLimit {
    fn from(error: sqlx::Error) -> Self {
        match error.as_database_error().and_then(|e| e.constraint()) {
            Some("cala_velocity_control_limits_velocity_control_id_velocity_l_key") => {
                Self::Domain(LimitAlreadyAddedToControl)
            }
            _ => Self::Sqlx(error),
        }
    }
}
