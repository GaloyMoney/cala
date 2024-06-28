use thiserror::Error;

#[derive(Error, Debug)]
pub enum VelocityError {
    #[error("VelocityError - Sqlx: {0}")]
    Sqlx(#[from] sqlx::Error),
    #[error("VelocityError - EntityError: {0}")]
    EntityError(#[from] crate::entity::EntityError),
}
