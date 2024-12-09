use rust_decimal::Decimal;
use thiserror::Error;

use cala_types::primitives::{Currency, Layer};
use cel_interpreter::CelError;

#[derive(Error, Debug)]
pub enum TxTemplateError {
    #[error("TxTemplateError - Sqlx: {0}")]
    Sqlx(sqlx::Error),
    #[error("TxTemplateError - DuplicateCode: code already exists")]
    DuplicateCode,
    #[error("TxTemplateError - DuplicateId: id already exists")]
    DuplicateId,
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
    #[error("{0}")]
    ParamError(#[from] crate::param::error::ParamError),
    #[error("TxTemplateError - EsEntityError: {0}")]
    EsEntityError(es_entity::EsEntityError),
    #[error("TxTemplateError - CursorDestructureError: {0}")]
    CursorDestructureError(#[from] es_entity::CursorDestructureError),
}

impl From<sqlx::Error> for TxTemplateError {
    fn from(e: sqlx::Error) -> Self {
        match e {
            sqlx::Error::Database(ref err) if err.is_unique_violation() => {
                let Some(constraint) = err.constraint() else {
                    return Self::Sqlx(e);
                };
                if constraint.contains("code") {
                    Self::DuplicateCode
                } else if constraint.contains("id") {
                    Self::DuplicateId
                } else {
                    Self::Sqlx(e)
                }
            }
            e => Self::Sqlx(e),
        }
    }
}

es_entity::from_es_entity_error!(TxTemplateError);
