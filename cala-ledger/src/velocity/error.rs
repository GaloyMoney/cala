use rust_decimal::Decimal;
use thiserror::Error;

use cel_interpreter::CelError;

use crate::primitives::*;

#[derive(Error, Debug)]
pub enum VelocityError {
    #[error("VelocityError - Sqlx: {0}")]
    Sqlx(#[from] sqlx::Error),
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
}

#[derive(Error, Debug)]
#[error("Velocity limit {limit_id} exceeded for account {account_id} - Limit: {currency} {limit}, Requested: {requested}")]
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
