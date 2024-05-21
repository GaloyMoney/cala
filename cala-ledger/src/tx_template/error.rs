use thiserror::Error;

use rust_decimal::Decimal;

use cala_types::primitives::{Currency, Layer};
use cel_interpreter::CelError;

use crate::outbox::error::OutboxError;

#[derive(Error, Debug)]
pub enum TxTemplateError {
    #[error("TxTemplateError - Sqlx: {0}")]
    Sqlx(#[from] sqlx::Error),
    #[error("{0}")]
    OutboxError(#[from] OutboxError),
    #[error("TxTemplateError - EntityError: {0}")]
    EntityError(#[from] crate::entity::EntityError),
    #[error("TxTemplateError - TxParamTypeMismatch: {0}")]
    TxParamTypeMismatch(String),
    #[error("SqlxLedgerError - TooManyParameters")]
    TooManyParameters,
    #[error("TxTemplateError - CelError: {0}")]
    CelError(#[from] CelError),
    #[error("TxTemplateError - NotFound")]
    NotFound,
    #[error("TxTemplateError - SerdeJson: {0}")]
    SerdeJson(#[from] serde_json::Error),
    #[error("TxTemplateError - UnbalancedTransaction: currency {0}, layer {1:?}, amount {2}")]
    UnbalancedTransaction(Currency, Layer, Decimal),
    #[error("TxTemplateError - NotFound: code '{0}' not found")]
    CouldNotFindByCode(String),
}
