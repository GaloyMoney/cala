use rust_decimal::Decimal;
use thiserror::Error;

use cala_types::primitives::{Currency, Layer};
use cel_interpreter::CelError;

use super::repo::{
    TxTemplateColumn, TxTemplateCreateError, TxTemplateFindError, TxTemplateModifyError,
    TxTemplateQueryError,
};

#[derive(Error, Debug)]
pub enum TxTemplateError {
    #[error("TxTemplateError - Sqlx: {0}")]
    Sqlx(#[from] sqlx::Error),
    #[error("TxTemplateError - Create: {0}")]
    Create(TxTemplateCreateError),
    #[error("TxTemplateError - Modify: {0}")]
    Modify(#[from] TxTemplateModifyError),
    #[error("TxTemplateError - Find: {0}")]
    Find(TxTemplateFindError),
    #[error("TxTemplateError - Query: {0}")]
    Query(#[from] TxTemplateQueryError),
    #[error("TxTemplateError - DuplicateCode: code '{0}' already exists")]
    DuplicateCode(String),
    #[error("TxTemplateError - DuplicateId: id '{0}' already exists")]
    DuplicateId(String),
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
}

impl From<TxTemplateFindError> for TxTemplateError {
    fn from(error: TxTemplateFindError) -> Self {
        match error {
            TxTemplateFindError::NotFound {
                column: Some(TxTemplateColumn::Code),
                value,
                ..
            } => Self::CouldNotFindByCode(value),
            other => Self::Find(other),
        }
    }
}

impl From<TxTemplateCreateError> for TxTemplateError {
    fn from(error: TxTemplateCreateError) -> Self {
        match error {
            TxTemplateCreateError::ConstraintViolation {
                column: Some(TxTemplateColumn::Code),
                value,
                ..
            } => Self::DuplicateCode(value.unwrap_or_default()),
            TxTemplateCreateError::ConstraintViolation {
                column: Some(TxTemplateColumn::Id),
                value,
                ..
            } => Self::DuplicateId(value.unwrap_or_default()),
            other => Self::Create(other),
        }
    }
}
