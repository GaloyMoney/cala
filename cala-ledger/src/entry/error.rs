use thiserror::Error;

#[derive(Error, Debug)]
pub enum EntryError {
    #[error("EntryError - Sqlx: {0}")]
    Sqlx(#[from] sqlx::Error),
    #[error("EntryError - EntityError: {0}")]
    EntityError(#[from] crate::entity::EntityError),
}
