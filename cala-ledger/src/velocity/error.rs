use rust_decimal::Decimal;
use thiserror::Error;

use cel_interpreter::CelError;

use crate::primitives::*;

#[derive(Error, Debug)]
pub enum VelocityError {
    #[error("VelocityError - Sqlx: {0}")]
    Sqlx(sqlx::Error),
    #[error("VelocityError - CelError: {0}")]
    CelError(#[from] CelError),
    #[error("{0}")]
    ParamError(#[from] crate::param::error::ParamError),
    #[error("VelocityError - Could not find control by id: {0}")]
    CouldNotFindControlById(VelocityControlId),
    #[error("VelocityError - Enforcement: {0}")]
    Enforcement(#[from] LimitExceededError),
    #[error("VelocityError - EsEntityError: {0}")]
    EsEntityError(es_entity::EsEntityError),
    #[error("VelocityError - CursorDestructureError: {0}")]
    CursorDestructureError(#[from] es_entity::CursorDestructureError),
    #[error("VelocityError - control_id already exists")]
    ControlIdAlreadyExists,
    #[error("VelocityError - limit_id already exists")]
    LimitIdAlreadyExists,
    #[error("VelocityError - Limit already added to Control")]
    LimitAlreadyAddedToControl,
}

impl From<sqlx::Error> for VelocityError {
    fn from(error: sqlx::Error) -> Self {
        if let Some(err) = error.as_database_error() {
            if let Some(constraint) = err.constraint() {
                if constraint.contains("cala_velocity_controls_pkey") {
                    return Self::ControlIdAlreadyExists;
                }
                if constraint.contains("cala_velocity_limits_pkey") {
                    return Self::LimitIdAlreadyExists;
                }
                if constraint
                    .contains("cala_velocity_control_limits_velocity_control_id_velocity_l_key")
                {
                    return Self::LimitAlreadyAddedToControl;
                }
            }
        }
        Self::Sqlx(error)
    }
}

#[derive(Error, Debug)]
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

es_entity::from_es_entity_error!(VelocityError);
