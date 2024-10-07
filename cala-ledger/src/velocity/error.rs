use rust_decimal::Decimal;
use thiserror::Error;

use cel_interpreter::CelError;

use crate::primitives::*;

#[derive(Error, Debug)]
pub enum VelocityError {
    #[error("VelocityError - Sqlx: {0}")]
    Sqlx(#[from] sqlx::Error),
    #[error("VelocityError - EntityError: {0}")]
    EntityError(#[from] crate::entity::EntityError),
    #[error("VelocityError - CelError: {0}")]
    CelError(#[from] CelError),
    #[error("{0}")]
    ParamError(#[from] crate::param::error::ParamError),
    #[error("VelocityError - Could not find control by id: {0}")]
    CouldNotFindControlById(VelocityControlId),
    #[error("VelocityError - Enforcement: {0}")]
    Enforcement(#[from] LimitExceededError),
}

#[derive(Error, Debug)]
#[error("Velocity limit exceeded for account {account_id} in currency {currency}: Limit ({limit_id}): {limit}, Requested: {requested}")]
pub struct LimitExceededError {
    pub account_id: AccountId,
    pub currency: String,
    pub limit_id: VelocityLimitId,
    pub layer: Layer,
    pub limit: Decimal,
    pub requested: Decimal,
}
