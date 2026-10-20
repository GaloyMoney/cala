pub(crate) use super::repo::{JournalConstraintViolation, JournalEnsureRejection};
use crate::primitives::*;
use es_entity::errlanes;

#[derive(Debug, errlanes::Rejection, errlanes::Lift)]
#[rejection(code = "CALA_JOURNAL_CODE_ALREADY_EXISTS")]
#[error("CodeAlreadyExists: {0:?}")]
#[lift(es_entity::ConstraintConflict<Option<String>>, field = attempted)]
pub struct JournalCodeAlreadyExists(pub Option<Option<String>>);

#[derive(Debug, errlanes::Rejection, errlanes::Lift)]
#[lift(JournalConstraintViolation)]
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

/// What [`Journals::ensure_in_op`](super::Journals::ensure_in_op) can reject.
#[derive(Debug, errlanes::Rejection, errlanes::Lift)]
#[lift(JournalEnsureRejection)]
pub enum EnsureJournalRejection {
    /// The new journal's id and `code` match two different stored journals.
    #[rejection(code = "CALA_JOURNAL_KEYS_SPAN_ENTITIES")]
    #[error("Journal keys {:?} match more than one journal: {:?}", keys, ids)]
    #[lift(JournalEnsureRejection::KeysSpanEntities, with = keys_span_journals)]
    KeysSpanEntities {
        ids: Vec<JournalId>,
        keys: Vec<&'static str>,
    },
}

fn keys_span_journals(rejection: JournalEnsureRejection) -> EnsureJournalRejection {
    let JournalEnsureRejection::KeysSpanEntities { ids, keys } = rejection;
    EnsureJournalRejection::KeysSpanEntities { ids, keys }
}
