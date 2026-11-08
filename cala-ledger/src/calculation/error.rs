pub(crate) use super::repo::CalculationConstraintViolation;
use crate::primitives::*;
use cala_types::calculation::CalculationBalanceId;
use es_entity::errlanes;

#[derive(Debug, errlanes::Rejection, errlanes::Lift)]
#[lift(CalculationConstraintViolation)]
pub enum CreateCalculationRejection {
    #[error("Calculation id already exists: {0}")]
    #[rejection(code = "CALA_CALCULATION_ID_ALREADY_EXISTS")]
    #[lift(CalculationConstraintViolation::Pkey, field = attempted)]
    IdAlreadyExists(CalculationId),
    #[error("Calculation code already exists: {0:?}")]
    #[rejection(code = "CALA_CALCULATION_CODE_ALREADY_EXISTS")]
    #[lift(CalculationConstraintViolation::CodeKey, field = attempted)]
    CodeAlreadyExists(Option<String>),
    #[error("Calculation journal not found: {0}")]
    #[rejection(code = "CALA_CALCULATION_JOURNAL_NOT_FOUND")]
    JournalNotFound(JournalId),
}

#[derive(Debug, errlanes::Rejection)]
#[rejection(code = "CALA_CALCULATION_NOT_FOUND")]
#[error("Calculation not found: {0}")]
pub struct CalculationNotFound(pub CalculationId);

#[derive(Debug, errlanes::Rejection)]
#[rejection(code = "CALA_CALCULATION_CODE_NOT_FOUND")]
#[error("Calculation code not found: {0}")]
pub struct CalculationCodeNotFound(pub String);

#[derive(Debug, errlanes::Rejection)]
pub enum AttachCalculationRejection {
    #[error("Calculation not found: {0}")]
    #[rejection(code = "CALA_CALCULATION_NOT_FOUND")]
    CalculationNotFound(CalculationId),
    #[error("Calculation {0} is global and cannot be attached")]
    #[rejection(code = "CALA_CALCULATION_GLOBAL_CANNOT_ATTACH")]
    GlobalCannotAttach(CalculationId),
    #[error("Calculation target not found: {0}")]
    #[rejection(code = "CALA_CALCULATION_TARGET_NOT_FOUND")]
    TargetNotFound(AccountId),
    #[error("Calculation target {target} is in journal {target_journal}, calculation is in journal {calculation_journal}")]
    #[rejection(code = "CALA_CALCULATION_JOURNAL_MISMATCH")]
    JournalMismatch {
        target: AccountId,
        target_journal: JournalId,
        calculation_journal: JournalId,
    },
    #[error("Calculation target already attached")]
    #[rejection(code = "CALA_CALCULATION_ALREADY_ATTACHED")]
    AlreadyAttached,
}

#[derive(Debug, errlanes::Rejection)]
#[rejection(code = "CALA_CALCULATION_BALANCE_NOT_FOUND")]
#[error("Calculation balance not found")]
pub struct CalculationBalanceNotFound(pub CalculationBalanceId);

/// The unit — the Global calculation, or the attachment of `account_id` —
/// is still being backfilled: its series hold a prefix of their history.
#[derive(Debug, errlanes::Rejection)]
#[rejection(code = "CALA_CALCULATION_BACKFILL_IN_PROGRESS")]
#[error("Calculation {calculation_id} backfill in progress for {account_id}")]
pub struct CalculationBackfillInProgress {
    pub calculation_id: CalculationId,
    pub account_id: AccountId,
}

/// A read of one calculation series.
#[derive(Debug, errlanes::Rejection)]
pub enum CalculationBalanceRejection {
    #[rejection(delegate, from)]
    #[error("{0}")]
    NotFound(CalculationBalanceNotFound),
    #[rejection(delegate, from)]
    #[error("{0}")]
    BackfillInProgress(CalculationBackfillInProgress),
}

#[derive(Debug, errlanes::Rejection)]
#[rejection(code = "CALA_CALCULATION_ALREADY_ATTACHED")]
#[error("Calculation target already attached")]
pub(crate) struct CalculationAlreadyAttached;

impl From<CalculationAlreadyAttached> for AttachCalculationRejection {
    fn from(_: CalculationAlreadyAttached) -> Self {
        Self::AlreadyAttached
    }
}

#[derive(Debug, errlanes::Classify)]
pub(crate) enum AttachWrite {
    #[classify(delegate)]
    Domain(CalculationAlreadyAttached),
    #[classify(delegate)]
    Sqlx(sqlx::Error),
}

impl From<sqlx::Error> for AttachWrite {
    fn from(error: sqlx::Error) -> Self {
        match error.as_database_error().and_then(|e| e.constraint()) {
            Some("cala_calculation_attachments_calculation_id_account_id_key") => {
                Self::Domain(CalculationAlreadyAttached)
            }
            _ => Self::Sqlx(error),
        }
    }
}

#[derive(Debug, errlanes::Rejection)]
#[rejection(code = "CALA_CALCULATION_BACKFILL_TIMEOUT")]
#[error("Calculation {calculation_id} backfill not complete after {elapsed:?}")]
pub struct CalculationBackfillTimeout {
    pub calculation_id: CalculationId,
    pub target: Option<AccountId>,
    pub elapsed: std::time::Duration,
}
