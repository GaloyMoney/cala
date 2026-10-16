use super::repo::{TxTemplateConstraintViolation, TxTemplateEnsureRejection};
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
/// reject: the new template's id and `code` name two different stored
/// templates.
#[derive(Debug, errlanes::Rejection, errlanes::Lift)]
#[lift(TxTemplateEnsureRejection)]
pub enum EnsureTxTemplateRejection {
    /// Carries the stored ids and the key column names, never values.
    #[rejection(code = "CALA_TX_TEMPLATE_KEYS_SPAN_ENTITIES")]
    #[error("Template keys {:?} match more than one template: {:?}", keys, ids)]
    #[lift(TxTemplateEnsureRejection::KeysSpanEntities, with = keys_span_tx_templates)]
    KeysSpanEntities {
        ids: Vec<TxTemplateId>,
        keys: Vec<&'static str>,
    },
}

fn keys_span_tx_templates(rejection: TxTemplateEnsureRejection) -> EnsureTxTemplateRejection {
    let TxTemplateEnsureRejection::KeysSpanEntities { ids, keys } = rejection;
    EnsureTxTemplateRejection::KeysSpanEntities { ids, keys }
}
