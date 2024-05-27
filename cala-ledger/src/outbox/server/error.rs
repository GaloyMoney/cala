use thiserror::Error;

#[derive(Error, Debug)]
pub enum OutboxServerError {
    #[error("OutboxServerError - TonicError: {0}")]
    TonicError(#[from] tonic::transport::Error),
}
