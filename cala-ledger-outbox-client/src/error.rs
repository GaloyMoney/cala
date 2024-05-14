use thiserror::Error;
use tonic::transport;

#[derive(Error, Debug)]
pub enum CalaLedgerOutboxClientError {
    #[error("CalaLedgerOutboxError - Connection: {0}")]
    ConnectionError(#[from] transport::Error),
    #[error("CalaLedgerOutboxError - Tonic: {0}")]
    TonicError(#[from] tonic::Status),
    #[error("CalaLedgerOutboxError - Tonic: {0}")]
    DecodeError(#[from] prost::DecodeError),
    #[error("CalaLedgerOutboxError - Uuid: {0}")]
    Uuid(#[from] uuid::Error),
    #[error("CalaLedgerOutboxError - Serde: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("CalaLedgerOutboxError - MissingField")]
    MissingField,
    #[error("CalaLedgerOutboxError - ParseTagError")]
    ParseTagError(#[from] cala_types::primitives::ParseTagError),
    #[error("CalaLedgerOutboxError - CelError: {0}")]
    CelError(#[from] cel_interpreter::CelError),
    #[error("CalaLedgerOutboxError - ChronoError: {0}")]
    ChronoError(#[from] chrono::ParseError),
}
