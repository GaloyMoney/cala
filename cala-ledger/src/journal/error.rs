use super::repo::JournalConstraintViolation;
use es_entity::errlanes;

#[derive(errlanes::Rejection, errlanes::Lift, Debug)]
#[lift(JournalConstraintViolation, unhandled = fatal)]
pub enum JournalRejection {
    #[error("journal '{0}' not found")]
    #[rejection(code = "CALA_JOURNAL_COULD_NOT_FIND_BY_ID")]
    CouldNotFindById(crate::JournalId),
    #[error("journal code '{0}' not found")]
    #[rejection(code = "CALA_JOURNAL_COULD_NOT_FIND_BY_CODE")]
    CouldNotFindByCode(String),
    #[error("duplicate id {0}")]
    #[rejection(code = "CALA_JOURNAL_DUPLICATE_ID")]
    #[lift(JournalConstraintViolation::Pkey, field = attempted)]
    DuplicateId(crate::JournalId),
    #[error("JournalRejection - code '{0:?}' already exists")]
    #[rejection(code = "CALA_JOURNAL_CODE_ALREADY_EXISTS")]
    #[lift(JournalConstraintViolation::CodeKey, field = attempted)]
    CodeAlreadyExists(Option<Option<String>>),
}
