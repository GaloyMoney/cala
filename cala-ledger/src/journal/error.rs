use thiserror::Error;

use super::repo::JournalConstraintViolation;
use crate::primitives::JournalId;

#[errlanes::compose]
#[derive(Debug, Clone, Error)]
#[lift(JournalLookupRejection, strict)]
#[lift(JournalConstraintViolation, unhandled = fatal)]
pub enum JournalRejection {
    #[lift(JournalLookupRejection::NotFoundById)]
    #[error("journal '{0}' not found")]
    NotFoundById(JournalId),
    #[lift(JournalLookupRejection::NotFoundByCode)]
    #[error("journal with code '{0}' not found")]
    NotFoundByCode(String),
    #[error("code '{0}' already exists")]
    #[lift(JournalConstraintViolation::CodeKey)]
    #[rejection(code = "CODE_ALREADY_EXISTS")]
    CodeAlreadyExists(#[source] es_entity::ConstraintConflict<Option<String>>),
}

pub type JournalError = errlanes::Fail<JournalRejection, crate::CalaLanes>;

#[derive(Debug, Clone, Error, errlanes::Rejection)]
pub enum JournalLookupRejection {
    #[error("journal '{0}' not found")]
    NotFoundById(JournalId),
    #[error("journal with code '{0}' not found")]
    NotFoundByCode(String),
}
pub type JournalLookupError = errlanes::Fail<JournalLookupRejection, crate::CalaLanes>;
