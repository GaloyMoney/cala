use super::{control::VelocityControlConstraintViolation, limit::VelocityLimitConstraintViolation};
use crate::param::ParamDefaultRejection;
use crate::primitives::*;
use cel_interpreter::*;
use es_entity::errlanes;
use rust_decimal::Decimal;

#[derive(errlanes::Rejection, Debug)]
#[rejection(code = "CALA_VELOCITY_LIMIT_EXCEEDED")]
#[error(
    "Velocity limit {limit_id} exceeded for account {account_id} - limit: {currency} {limit}, requested: {requested}, layer: {layer:?}, direction: {direction:?}"
)]
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

/// Failures from binding parameters and evaluating limits while attaching a control.
#[derive(Debug, errlanes::Rejection)]
pub enum AttachVelocityControlRejection {
    #[rejection(code = "CALA_VELOCITY_COULD_NOT_FIND_CONTROL_BY_ID")]
    #[error("Velocity control not found: {0}")]
    ControlNotFound(VelocityControlId),
    /// Coercing a supplied parameter to its declared type.
    #[error("{0}")]
    #[rejection(code = "CALA_VELOCITY_PARAMETER_INVALID", from)]
    Param(cala_types::param::ParamValueRejection),
    /// Evaluating and coercing the default for an omitted parameter.
    #[error("{0}")]
    #[rejection(code = "CALA_VELOCITY_PARAMETER_DEFAULT_FAILED")]
    Default(#[source] ParamDefaultRejection),
    /// Evaluating a balance-limit field, including result conversion.
    #[error("{0}")]
    #[rejection(code = "CALA_VELOCITY_LIMIT_EVALUATION_FAILED", from)]
    Cel(CelConversionRejection),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limit_exceeded_error_includes_velocity_context() {
        let limit_id = VelocityLimitId::new();
        let account_id = AccountId::new();
        let err = LimitExceededError {
            account_id,
            currency: "USD".parse().expect("USD currency"),
            limit_id,
            layer: Layer::Settled,
            direction: DebitOrCredit::Debit,
            limit: Decimal::ZERO,
            requested: Decimal::new(155, 0),
        };

        let message = err.to_string();
        assert!(message.contains(&limit_id.to_string()));
        assert!(message.contains(&account_id.to_string()));
        assert!(message.contains("USD"));
        assert!(message.contains("155"));
        assert!(message.contains("Settled"));
        assert!(message.contains("Debit"));
    }
}
