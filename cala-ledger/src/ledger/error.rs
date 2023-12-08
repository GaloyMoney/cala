use thiserror::Error;

#[derive(Error, Debug)]
pub enum LedgerError {
    #[error("LedgerError - Sqlx: {0}")]
    Sqlx(#[from] sqlx::Error),
    #[error("LedgerError - Migrate: {0}")]
    SqlxMigrate(#[from] sqlx::migrate::MigrateError),
    #[error("LedgerError - Config: {0}")]
    ConfigError(String),
}
