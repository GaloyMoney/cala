use thiserror::Error;

use cala_types::tx_template::ParamDataType;
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
    #[error("TxTemplateError - TxParamTypeMismatch: expected {0:?}")]
    TxParamTypeMismatch(ParamDataType),
    #[error("SqlxLedgerError - TooManyParameters")]
    TooManyParameters,
    #[error("TxTemplateError - CelError: {0}")]
    CelError(#[from] CelError),
    #[error("TxTemplateError - NotFound")]
    NotFound,
    #[error("TxTemplateError - SerdeJson: {0}")]
    SerdeJson(#[from] serde_json::Error),
}
