use thiserror::Error;

use crate::import_job::error::*;

#[derive(Error, Debug)]
pub enum ApplicationError {
    #[error("ApplicationError - Sqlx: {0}")]
    Sqlx(#[from] sqlx::Error),
    #[error("ApplicationError - ImportJobError: {0}")]
    ImportJob(#[from] ImportJobError),
}
