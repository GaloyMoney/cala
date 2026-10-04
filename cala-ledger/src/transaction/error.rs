use es_entity::errlanes;

use cala_types::primitives::TransactionId;

#[derive(errlanes::Rejection, Debug)]
pub enum TransactionRejection {
    #[error("TransactionRejection - NotFound: id '{0}' not found")]
    #[rejection(code = "CALA_TRANSACTION_COULD_NOT_FIND_BY_ID")]
    CouldNotFindById(TransactionId),
    #[error("TransactionRejection - NotFound: external id '{0}' not found")]
    #[rejection(code = "CALA_TRANSACTION_COULD_NOT_FIND_BY_EXTERNAL_ID")]
    CouldNotFindByExternalId(String),
}
