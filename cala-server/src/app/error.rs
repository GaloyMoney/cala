use thiserror::Error;

use crate::job::error::*;

#[derive(Error, Debug)]
pub enum ApplicationError {
    #[error("ApplicationError - Sqlx: {0}")]
    Sqlx(#[from] sqlx::Error),
    #[error("ApplicationError - Job: {0}")]
    Job(#[from] JobError),
}
