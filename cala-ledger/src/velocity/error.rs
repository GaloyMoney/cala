use thiserror::Error;

use cel_interpreter::CelError;

#[derive(Error, Debug)]
pub enum VelocityError {
    #[error("VelocityError - Sqlx: {0}")]
    Sqlx(#[from] sqlx::Error),
    #[error("VelocityError - EntityError: {0}")]
    EntityError(#[from] crate::entity::EntityError),
    #[error("VelocityError - CelError: {0}")]
    CelError(#[from] CelError),
    #[error("{0}")]
    ParamError(#[from] crate::param::error::ParamError),
}
