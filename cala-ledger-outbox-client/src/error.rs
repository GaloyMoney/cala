use thiserror::Error;
use tonic::transport;

#[derive(Error, Debug)]
pub enum CalaLedgerOutboxClientError {
    #[error("CalaLedgerOutboxError - Connection: {0}")]
    ConnectionError(#[from] transport::Error),
    #[error("CalaLedgerOutboxError - Tonic: {0}")]
    TonicError(#[from] tonic::Status),
}
