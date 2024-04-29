use thiserror::Error;

#[derive(Error, Debug)]
pub enum ImportJobError {
    #[error("ImportJobError - Sqlx: {0}")]
    Sqlx(#[from] sqlx::Error),
    #[error("ImportJobError - EntityError: {0}")]
    EntityError(#[from] cala_ledger::entity::EntityError),
}
