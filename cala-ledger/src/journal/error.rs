use thiserror::Error;

use crate::error_support::{impl_lane_error_fail, impl_lane_error_fault};

use super::repo::{JournalConstraint, JournalConstraintViolation};
use crate::primitives::JournalId;

#[derive(Debug, Clone, Error, errlanes::Rejection)]
#[rejection(lift(JournalConstraintViolation))]
pub enum JournalRejection {
    #[error("journal '{0}' not found")]
    NotFoundById(JournalId),
    #[error("journal with code '{0}' not found")]
    NotFoundByCode(String),
    #[error("code '{0}' already exists")]
    #[rejection(key = JournalConstraint::CodeKey, with = code_taken)]
    CodeAlreadyExists(String),
}

fn code_taken(cv: JournalConstraintViolation) -> JournalRejection {
    JournalRejection::CodeAlreadyExists(cv.value().unwrap_or_default().to_owned())
}

#[derive(Debug, Error)]
pub enum JournalError {
    #[error(transparent)]
    Rejected(#[from] JournalRejection),
    #[error(transparent)]
    Transient(#[from] errlanes::Transient),
    #[error(transparent)]
    Fatal(#[from] errlanes::Fatal),
}

impl_lane_error_fault!(JournalError);
impl_lane_error_fail!(JournalError, JournalRejection, JournalConstraintViolation);
