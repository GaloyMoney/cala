use crate::primitives::TransactionId;
use es_entity::errlanes;
#[derive(Debug, errlanes::Rejection)]
#[rejection(code = "CALA_TRANSACTION_COULD_NOT_FIND_BY_ID")]
#[error("Transaction not found: {0}")]
pub struct TransactionNotFound(pub TransactionId);

#[derive(Debug, errlanes::Rejection)]
#[rejection(code = "CALA_TRANSACTION_COULD_NOT_FIND_BY_EXTERNAL_ID")]
#[error("Transaction external id not found: {0}")]
pub struct TransactionExternalIdNotFound(pub String);
