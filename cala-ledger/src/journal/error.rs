use super::repo::JournalConstraintViolation;
use crate::primitives::*;
use es_entity::errlanes;

#[derive(Debug, errlanes::Rejection, errlanes::Lift)]
#[lift(JournalConstraintViolation, unhandled = fatal)]
pub enum CreateJournalRejection {
    #[rejection(code = "CALA_JOURNAL_DUPLICATE_ID")]
    #[error("DuplicateId: {0:?}")]
    #[lift(JournalConstraintViolation::Pkey, field = attempted)]
    DuplicateId(JournalId),
    #[rejection(code = "CALA_JOURNAL_CODE_ALREADY_EXISTS")]
    #[error("CodeAlreadyExists: {0:?}")]
    #[lift(JournalConstraintViolation::CodeKey, field = attempted)]
    CodeAlreadyExists(Option<Option<String>>),
}

#[derive(Debug, errlanes::Rejection, errlanes::Lift)]
#[lift(JournalConstraintViolation, unhandled = fatal)]
pub enum PersistJournalRejection {
    #[rejection(forward = CreateJournalRejection::CodeAlreadyExists)]
    #[error("CodeAlreadyExists: {0:?}")]
    #[lift(JournalConstraintViolation::CodeKey, field = attempted)]
    CodeAlreadyExists(Option<Option<String>>),
}

#[derive(Debug, errlanes::Rejection)]
#[rejection(code = "CALA_JOURNAL_COULD_NOT_FIND_BY_ID")]
#[error("Journal not found: {0}")]
pub struct JournalNotFound(pub JournalId);

#[derive(Debug, errlanes::Rejection)]
#[rejection(code = "CALA_JOURNAL_COULD_NOT_FIND_BY_CODE")]
#[error("Journal code not found: {0}")]
pub struct JournalCodeNotFound(pub String);
