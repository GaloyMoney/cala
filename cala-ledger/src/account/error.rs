use thiserror::Error;

#[derive(Error, Debug)]
pub enum AccountError {
    #[error("AccountError - Sqlx: {0}")]
    Sqlx(#[from] sqlx::Error),
}
