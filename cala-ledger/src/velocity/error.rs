use rust_decimal::Decimal;
use thiserror::Error;

use cel_interpreter::CelError;

use crate::primitives::*;

use super::control::{
    VelocityControlColumn, VelocityControlCreateError, VelocityControlFindError,
    VelocityControlModifyError, VelocityControlQueryError,
};
use super::limit::{
    VelocityLimitColumn, VelocityLimitCreateError, VelocityLimitFindError,
    VelocityLimitModifyError, VelocityLimitQueryError,
};

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
    #[error("VelocityError - HydrationError: {0}")]
    HydrationError(#[from] es_entity::EntityHydrationError),
    #[error("VelocityError - VelocityControlCreate: {0}")]
    VelocityControlCreate(VelocityControlCreateError),
    #[error("VelocityError - VelocityControlModify: {0}")]
    VelocityControlModify(#[from] VelocityControlModifyError),
    #[error("VelocityError - VelocityControlFind: {0}")]
    VelocityControlFind(#[from] VelocityControlFindError),
    #[error("VelocityError - VelocityControlQuery: {0}")]
    VelocityControlQuery(#[from] VelocityControlQueryError),
    #[error("VelocityError - VelocityLimitCreate: {0}")]
    VelocityLimitCreate(VelocityLimitCreateError),
    #[error("VelocityError - VelocityLimitModify: {0}")]
    VelocityLimitModify(#[from] VelocityLimitModifyError),
    #[error("VelocityError - VelocityLimitFind: {0}")]
    VelocityLimitFind(#[from] VelocityLimitFindError),
    #[error("VelocityError - VelocityLimitQuery: {0}")]
    VelocityLimitQuery(#[from] VelocityLimitQueryError),
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

impl From<VelocityControlCreateError> for VelocityError {
    fn from(error: VelocityControlCreateError) -> Self {
        if error.was_duplicate(VelocityControlColumn::Id) {
            return Self::ControlIdAlreadyExists;
        }
        Self::VelocityControlCreate(error)
    }
}

impl From<VelocityLimitCreateError> for VelocityError {
    fn from(error: VelocityLimitCreateError) -> Self {
        if error.was_duplicate(VelocityLimitColumn::Id) {
            return Self::LimitIdAlreadyExists;
        }
        Self::VelocityLimitCreate(error)
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
