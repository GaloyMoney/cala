use rust_decimal::Decimal;
use sqlx::error::DatabaseError;
use thiserror::Error;

use cala_types::primitives::{Currency, Layer};
use cel_interpreter::CelError;

#[derive(Error, Debug)]
pub enum TxTemplateError {
    #[error("TxTemplateError - Sqlx: {0}")]
    Sqlx(sqlx::Error),
    #[error("TxTemplateError - DuplicateKey: {0}")]
    DuplicateKey(Box<dyn DatabaseError>),
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

impl From<sqlx::Error> for TxTemplateError {
    fn from(e: sqlx::Error) -> Self {
        match e {
            sqlx::Error::Database(err) if err.message().contains("duplicate key") => {
                Self::DuplicateKey(err)
            }
            e => Self::Sqlx(e),
        }
    }
}
