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
    Find(#[from] TxTemplateFindError),
    #[error("TxTemplateError - Query: {0}")]
    Query(#[from] TxTemplateQueryError),
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
}

impl From<TxTemplateCreateError> for TxTemplateError {
    fn from(error: TxTemplateCreateError) -> Self {
        if error.was_duplicate(TxTemplateColumn::Code) {
            return Self::DuplicateCode;
        }
        if error.was_duplicate(TxTemplateColumn::Id) {
            return Self::DuplicateId;
        }
        Self::Create(error)
    }
}
