use thiserror::Error;

#[derive(Error, Debug)]
pub enum JobExecutorError {
    #[error("JobExecutorError - Sqlx: {0}")]
    Sqlx(#[from] sqlx::Error),
    #[error("JobExecutorError - InvalidPollInterval: {0}")]
    InvalidPollInterval(String),
}
