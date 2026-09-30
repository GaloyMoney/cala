use thiserror::Error;

use super::repo::TransactionConstraintViolation;
use cala_types::primitives::TransactionId;

#[derive(Debug, Clone, Error, errlanes::Rejection, errlanes::Lift)]
#[lift(TransactionConstraintViolation, unhandled = fatal)]
pub enum TransactionRejection {
    #[error("transaction '{0}' not found")]
    NotFoundById(TransactionId),
    #[error("transaction with external id '{0}' not found")]
    NotFoundByExternalId(String),
    #[error("external_id '{0}' already exists")]
    #[lift(TransactionConstraintViolation::ExternalIdKey)]
    #[rejection(code = "DUPLICATE_EXTERNAL_ID")]
    DuplicateExternalId(#[source] es_entity::ConstraintConflict<Option<String>>),
    #[error("id '{0}' already exists")]
    #[lift(TransactionConstraintViolation::Pkey)]
    #[rejection(code = "DUPLICATE_ID")]
    DuplicateId(#[source] es_entity::ConstraintConflict<TransactionId>),
}

pub type TransactionError = errlanes::Fail<TransactionRejection, crate::CalaLanes>;
