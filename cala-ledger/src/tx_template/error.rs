use super::repo::{TxTemplateConstraintViolation, TxTemplateEnsureByCodeRejection};
use cala_types::primitives::*;
use es_entity::errlanes;

#[derive(Debug, errlanes::Rejection, errlanes::Lift)]
#[lift(TxTemplateConstraintViolation)]
pub enum CreateTxTemplateRejection {
    #[error("template id already exists: {0}")]
    #[rejection(code = "CALA_TX_TEMPLATE_DUPLICATE_ID")]
    #[lift(TxTemplateConstraintViolation::Pkey, field = attempted)]
    DuplicateId(TxTemplateId),
    #[error("template code already exists: {0:?}")]
    #[rejection(code = "CALA_TX_TEMPLATE_DUPLICATE_CODE")]
    #[lift(TxTemplateConstraintViolation::CodeKey, field = attempted)]
    DuplicateCode(Option<String>),
}

#[derive(Debug, errlanes::Rejection)]
#[rejection(code = "CALA_TX_TEMPLATE_COULD_NOT_FIND_BY_CODE")]
#[error("Template code '{0}' not found")]
pub struct TxTemplateNotFound(pub String);

/// What [`TxTemplates::ensure_in_op`](super::TxTemplates::ensure_in_op) can
/// still reject on once a stored template with the same `code` is `Found`:
/// only a caller-chosen id that is already taken by a template with another
/// code.
#[derive(Debug, errlanes::Rejection, errlanes::Lift)]
#[lift(TxTemplateEnsureByCodeRejection)]
pub enum EnsureTxTemplateRejection {
    #[error("template id already exists: {0}")]
    #[rejection(code = "CALA_TX_TEMPLATE_DUPLICATE_ID")]
    #[lift(TxTemplateEnsureByCodeRejection::Pkey, field = attempted)]
    DuplicateId(TxTemplateId),
}
