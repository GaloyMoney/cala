use super::control::VelocityControlConstraintViolation;
use super::limit::VelocityLimitConstraintViolation;
use crate::param::error::ParamRejection;
use crate::primitives::*;
use rust_decimal::Decimal;
use thiserror::Error;

/// Control and limit management, distinct from posting enforcement.
#[errlanes::compose]
#[derive(Debug, Error)]
#[lift(ParamRejection, strict)]
#[lift(VelocityControlConstraintViolation, unhandled = fatal)]
#[lift(VelocityLimitConstraintViolation, unhandled = fatal)]
pub enum VelocityRejection {
    #[lift(ParamRejection::ParamTypeMismatch)]
    #[error("ParamError - ParamTypeMismatch: {0}")]
    ParamTypeMismatch(String),
    #[lift(ParamRejection::CelError)]
    #[error("ParamError - CelError: {0}")]
    CelError(#[source] cel_interpreter::CelError),
    #[error("velocity control '{0}' not found")]
    NotFoundControlById(VelocityControlId),
    #[error("control ID already exists: {0}")]
    #[lift(VelocityControlConstraintViolation::Pkey)]
    #[rejection(code = "CONTROL_ID_ALREADY_EXISTS")]
    ControlIdAlreadyExists(#[source] es_entity::ConstraintConflict<VelocityControlId>),
    #[error("limit ID already exists: {0}")]
    #[lift(VelocityLimitConstraintViolation::Pkey)]
    #[rejection(code = "LIMIT_ID_ALREADY_EXISTS")]
    LimitIdAlreadyExists(#[source] es_entity::ConstraintConflict<VelocityLimitId>),
    #[error("limit already added to control")]
    LimitAlreadyAddedToControl,
}
impl From<cel_interpreter::CelError> for VelocityRejection {
    fn from(error: cel_interpreter::CelError) -> Self {
        crate::param::error::ParamRejection::from(error).into()
    }
}

#[derive(Debug, Error, errlanes::Rejection)]
pub enum VelocityEnforcementRejection {
    #[error("velocity limit exceeded: {0}")]
    #[rejection(code = "ENFORCEMENT")]
    LimitExceeded(#[from] LimitExceededError),
    #[error("velocity evaluation failed: {0}")]
    #[rejection(code = "CEL_ERROR")]
    Evaluation(#[from] cel_interpreter::CelError),
}

pub type VelocityError = errlanes::Fail<VelocityRejection, crate::CalaLanes>;
pub type VelocityEnforcementError = errlanes::Fail<VelocityEnforcementRejection, crate::CalaLanes>;

pub(super) fn attach_limit_error(error: sqlx::Error) -> VelocityError {
    if let sqlx::Error::Database(db) = &error {
        if db.is_unique_violation()
            && db.constraint()
                == Some("cala_velocity_control_limits_velocity_control_id_velocity_l_key")
        {
            return VelocityRejection::LimitAlreadyAddedToControl.into();
        }
    }
    error.into()
}

#[derive(Debug, Clone, Error)]
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
