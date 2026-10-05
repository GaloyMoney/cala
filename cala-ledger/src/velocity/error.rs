use super::{control::VelocityControlConstraintViolation, limit::VelocityLimitConstraintViolation};
use crate::primitives::*;
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

/// Failures owned by velocity enforcement, independent of its posting caller.
#[derive(Debug, errlanes::Rejection)]
pub enum EnforceVelocityRejection {
    #[error("{0}")]
    #[rejection(delegate, from)]
    Cel(CelConversionRejection),
    #[error("{0}")]
    #[rejection(delegate, from)]
    LimitExceeded(LimitExceededError),
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

#[errlanes::compose]
#[derive(Debug)]
pub enum AttachVelocityControlRejection {
    #[rejection(code = "CALA_VELOCITY_COULD_NOT_FIND_CONTROL_BY_ID")]
    #[error("Velocity control not found: {0}")]
    ControlNotFound(VelocityControlId),
    #[rejection(code = "CEL_ERROR")]
    #[error("Error evaluating expression '{}': {}", expression, source)]
    DefaultUnknownIdent {
        expression: String,
        #[source]
        source: CelExecutionError,
    },
    #[rejection(code = "CEL_ERROR")]
    #[error("Error evaluating expression '{}': {}", expression, source)]
    DefaultMissingArgument {
        expression: String,
        #[source]
        source: CelExecutionError,
    },
    #[rejection(code = "CEL_ERROR")]
    #[error("Error evaluating expression '{}': {}", expression, source)]
    DefaultNoMatchingOverload {
        expression: String,
        #[source]
        source: CelExecutionError,
    },
    #[rejection(code = "CEL_ERROR")]
    #[error("Error evaluating expression '{}': {}", expression, source)]
    DefaultUnexpected {
        expression: String,
        #[source]
        source: CelExecutionError,
    },
    #[rejection(code = "CEL_ERROR")]
    #[error("Unsupported opaque value {} in '{}'", type_name, expression)]
    DefaultUnsupportedOpaque {
        expression: String,
        type_name: String,
    },
    #[rejection(code = "CEL_ERROR")]
    #[error("Could not downcast {} in '{}'", type_name, expression)]
    DefaultOpaqueDowncast {
        expression: String,
        type_name: &'static str,
    },
    #[rejection(code = "CEL_ERROR")]
    #[error("Cannot convert function value in '{}'", expression)]
    DefaultFunctionValue { expression: String },
    #[compose(flatten)]
    Param(cala_types::param::ParamValueRejection),
    #[compose(flatten)]
    Cel(cel_interpreter::CelConversionRejection),
}

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

impl AttachVelocityControlRejection {
    pub(crate) fn from_default(rejection: CelConversionRejection) -> Self {
        match rejection {
            CelConversionRejection::UnknownIdent { expression, source } => {
                Self::DefaultUnknownIdent { expression, source }
            }
            CelConversionRejection::MissingArgument { expression, source } => {
                Self::DefaultMissingArgument { expression, source }
            }
            CelConversionRejection::NoMatchingOverload { expression, source } => {
                Self::DefaultNoMatchingOverload { expression, source }
            }
            CelConversionRejection::Unexpected { expression, source } => {
                Self::DefaultUnexpected { expression, source }
            }
            CelConversionRejection::UnsupportedOpaque {
                expression,
                type_name,
            } => Self::DefaultUnsupportedOpaque {
                expression,
                type_name,
            },
            CelConversionRejection::OpaqueDowncast {
                expression,
                type_name,
            } => Self::DefaultOpaqueDowncast {
                expression,
                type_name,
            },
            CelConversionRejection::FunctionValue { expression } => {
                Self::DefaultFunctionValue { expression }
            }
            error @ (CelConversionRejection::CoreTypeCoercion(_)
            | CelConversionRejection::ExternalTypeCoercion(_)
            | CelConversionRejection::ExternalParse(_)
            | CelConversionRejection::Json(_)) => Self::from(error),
        }
    }
}
