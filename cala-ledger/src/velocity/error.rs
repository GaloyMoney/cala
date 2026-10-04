use super::control::VelocityControlConstraintViolation;
use super::limit::VelocityLimitConstraintViolation;
use es_entity::errlanes;
use rust_decimal::Decimal;

use cel_interpreter::CelError;

use crate::primitives::*;

#[derive(errlanes::Rejection, errlanes::Lift, Debug)]
#[lift(VelocityControlConstraintViolation, unhandled = fatal)]
#[lift(VelocityLimitConstraintViolation, unhandled = fatal)]
pub enum VelocityRejection {
    #[error("VelocityRejection - CelError: {0}")]
    #[rejection(delegate, from)]
    CelError(CelError),
    #[error("{0}")]
    #[rejection(delegate, from)]
    ParamRejection(crate::param::error::ParamRejection),
    #[error("VelocityRejection - Could not find control by id: {0}")]
    #[rejection(code = "CALA_VELOCITY_COULD_NOT_FIND_CONTROL_BY_ID")]
    CouldNotFindControlById(VelocityControlId),
    #[error("VelocityRejection - Enforcement: {0}")]
    #[rejection(delegate, from)]
    Enforcement(LimitExceededError),
    #[error("VelocityRejection - control_id '{0:?}' already exists")]
    #[rejection(code = "CALA_VELOCITY_CONTROL_ID_ALREADY_EXISTS")]
    #[lift(VelocityControlConstraintViolation::Pkey, field = attempted)]
    ControlIdAlreadyExists(VelocityControlId),
    #[error("VelocityRejection - limit_id '{0:?}' already exists")]
    #[rejection(code = "CALA_VELOCITY_LIMIT_ID_ALREADY_EXISTS")]
    #[lift(VelocityLimitConstraintViolation::Pkey, field = attempted)]
    LimitIdAlreadyExists(VelocityLimitId),
    #[error("VelocityRejection - Limit already added to Control")]
    #[rejection(code = "CALA_VELOCITY_LIMIT_ALREADY_ADDED_TO_CONTROL")]
    LimitAlreadyAddedToControl,
}

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

/// Classifies this write's known constraints; every other SQL failure keeps its native lane.
#[derive(Debug, errlanes::Classify)]
pub(crate) enum AttachLimit {
    #[classify(delegate)]
    Domain(VelocityRejection),
    #[classify(delegate)]
    Sqlx(sqlx::Error),
}
impl From<sqlx::Error> for AttachLimit {
    fn from(error: sqlx::Error) -> Self {
        match error.as_database_error().and_then(|e| e.constraint()) {
            Some("cala_velocity_control_limits_velocity_control_id_velocity_l_key") => {
                Self::Domain(VelocityRejection::LimitAlreadyAddedToControl)
            }
            _ => Self::Sqlx(error),
        }
    }
}
