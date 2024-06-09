use thiserror::Error;

use super::entity::JobType;

#[derive(Error, Debug)]
pub enum JobError {
    #[error("JobError - Sqlx: {0}")]
    Sqlx(#[from] sqlx::Error),
    #[error("JobError - EntityError: {0}")]
    EntityError(#[from] cala_ledger::entity::EntityError),
    #[error("JobError - InvalidPollInterval: {0}")]
    InvalidPollInterval(String),
    #[error("JobError - InvalidJobType: expected '{0}' but initializer was '{1}'")]
    JobTypeMismatch(JobType, JobType),
    #[error("JobError - JobInitError: {0}")]
    JobInitError(String),
    #[error("JobError - BadData: {0}")]
    CouldNotSerializeData(serde_json::Error),
    #[error("JobError - BadState: {0}")]
    CouldNotSerializeState(serde_json::Error),
    #[error("JobError - NoInitializerPresent")]
    NoInitializerPresent,
}
