use thiserror::Error;

use crate::outbox::{error::OutboxError, server::error::OutboxServerError};

#[derive(Error, Debug)]
pub enum LedgerError {
    #[error("LedgerError - Sqlx: {0}")]
    Sqlx(#[from] sqlx::Error),
    #[error("LedgerError - Migrate: {0}")]
    SqlxMigrate(#[from] sqlx::migrate::MigrateError),
    #[error("LedgerError - Config: {0}")]
    ConfigError(String),
    #[error("LedgerError - Outbox: {0}")]
    Outbox(#[from] OutboxError),
    #[error("LedgerError - OutboxServer: {0}")]
    OutboxServer(#[from] OutboxServerError),
}
