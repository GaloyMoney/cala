use thiserror::Error;

#[derive(Error, Debug)]
pub enum JobExecutionError {
    #[error("JobExecutionError - Sqlx: {0}")]
    Sqlx(#[from] sqlx::Error),
    #[error("JobExecutionError - InvalidPollInterval: {0}")]
    InvalidPollInterval(String),
}
