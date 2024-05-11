use thiserror::Error;

use super::entity::JobType;

#[derive(Error, Debug)]
pub enum JobExecutorError {
    #[error("JobExecutorError - Sqlx: {0}")]
    Sqlx(#[from] sqlx::Error),
    #[error("JobExecutorError - InvalidPollInterval: {0}")]
    InvalidPollInterval(String),
    #[error("JobExecutorError - InvalidJobType: {0}")]
    InvalidJobType(JobType),
    #[error("JobExecutorError - JobInitError: {0}")]
    JobInitError(String),
    #[error("JobExecutorError - BadConfig: {0}")]
    CouldNotSerializeConfig(serde_json::Error),
}
