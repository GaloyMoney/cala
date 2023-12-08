use thiserror::Error;

#[derive(Error, Debug)]
pub enum LedgerError {
    #[error("LedgerError - Migrate: {0}")]
    Sqlx(#[from] sqlx::migrate::MigrateError),
}
