use thiserror::Error;

use crate::outbox::OutboxError;

#[allow(clippy::large_enum_variant)]
#[derive(Error, Debug)]
pub enum OutboxServerError {
    #[error("OutboxServerError - TonicError: {0}")]
    TonicError(#[from] tonic::transport::Error),
    #[error("OutboxServerError - AppError: {0}")]
    AppError(#[from] OutboxError),
}
