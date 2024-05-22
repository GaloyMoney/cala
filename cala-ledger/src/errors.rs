pub use terrors::OneOf;
use thiserror::Error;

use cala_types::primitives::{Currency, Layer};
use rust_decimal::Decimal;

#[derive(Error, Debug)]
#[error("ConfigError: {0}")]
pub struct ConfigError(pub String);

#[derive(Error, Debug)]
#[error("Unexpected Db error: {0}")]
pub struct UnexpectedDbError(#[from] pub sqlx::Error);

#[derive(Error, Debug)]
#[error("Constraint violation: {0}")]
pub struct ConstraintVioliation(String);

#[derive(Error, Debug)]
#[error("DbMigrationError: {0}")]
pub struct DbMigrationError(#[from] pub sqlx::migrate::MigrateError);

#[derive(Error, Debug)]
#[error("Could not hydrate entity: {0}")]
pub struct HydratingEntityError(#[from] pub derive_builder::UninitializedFieldError);

#[derive(Error, Debug)]
#[error("Error evaluating cel expression: {0}")]
pub struct CelEvaluationError(pub Box<dyn std::error::Error>);

#[derive(Error, Debug)]
#[error("Entity not found")]
pub struct EntityNotFound;

#[derive(Error, Debug)]
#[error("TxParamTypeMismatch: {0}")]
pub struct TxParamTypeMismatch(pub String);

#[derive(Error, Debug)]
#[error("TooManyParams")]
pub struct TooManyParams;

#[derive(Error, Debug)]
#[error("Could not execute transaction due to balance locking issue")]
pub struct OptimisticLockingError;

#[derive(Error, Debug)]
#[error("UnbalancedTransaction: currency {0}, layer {1:?}, amount {2}")]
pub struct UnbalancedTransaction(pub Currency, pub Layer, pub Decimal);
