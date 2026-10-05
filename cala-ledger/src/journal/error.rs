pub(crate) use super::repo::JournalConstraintViolation;
use crate::primitives::*;
use es_entity::errlanes;

#[derive(Debug, errlanes::Rejection)]
#[rejection(code = "CALA_JOURNAL_CODE_ALREADY_EXISTS")]
#[error("CodeAlreadyExists: {0:?}")]
pub struct JournalCodeAlreadyExists(pub Option<Option<String>>);

impl From<es_entity::ConstraintConflict<Option<String>>> for JournalCodeAlreadyExists {
    fn from(conflict: es_entity::ConstraintConflict<Option<String>>) -> Self {
        Self(conflict.attempted)
    }
}

#[derive(Debug, errlanes::Rejection, errlanes::Lift)]
#[lift(JournalConstraintViolation, unhandled = fatal)]
pub enum CreateJournalRejection {
    #[rejection(code = "CALA_JOURNAL_DUPLICATE_ID")]
    #[error("DuplicateId: {0:?}")]
    #[lift(JournalConstraintViolation::Pkey, field = attempted)]
    DuplicateId(JournalId),
    #[rejection(delegate, from)]
    #[error("{0}")]
    #[lift(JournalConstraintViolation::CodeKey, into)]
    CodeAlreadyExists(JournalCodeAlreadyExists),
}

#[derive(Debug, errlanes::Rejection, errlanes::Lift)]
#[lift(JournalConstraintViolation, unhandled = fatal)]
pub enum PersistJournalRejection {
    #[rejection(delegate, from)]
    #[error("{0}")]
    #[lift(JournalConstraintViolation::CodeKey, into)]
    CodeAlreadyExists(JournalCodeAlreadyExists),
}

#[derive(Debug, errlanes::Rejection)]
#[rejection(code = "CALA_JOURNAL_COULD_NOT_FIND_BY_ID")]
#[error("Journal not found: {0}")]
pub struct JournalNotFound(pub JournalId);

#[derive(Debug, errlanes::Rejection)]
#[rejection(code = "CALA_JOURNAL_COULD_NOT_FIND_BY_CODE")]
#[error("Journal code not found: {0}")]
pub struct JournalCodeNotFound(pub String);
