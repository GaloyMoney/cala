use thiserror::Error;

use crate::error_support::{impl_lane_error_fail, impl_lane_error_fault};

use super::repo::{TransactionConstraint, TransactionConstraintViolation};
use cala_types::primitives::TransactionId;

#[derive(Debug, Clone, Error, errlanes::Rejection)]
#[rejection(lift(TransactionConstraintViolation))]
pub enum TransactionRejection {
    #[error("transaction '{0}' not found")]
    NotFoundById(TransactionId),
    #[error("transaction with external id '{0}' not found")]
    NotFoundByExternalId(String),
    #[error("external_id '{0}' already exists")]
    #[rejection(key = TransactionConstraint::ExternalIdKey, with = external_id_taken)]
    DuplicateExternalId(String),
    #[error("id '{0}' already exists")]
    #[rejection(key = TransactionConstraint::Pkey, with = id_taken)]
    DuplicateId(String),
}

fn external_id_taken(cv: TransactionConstraintViolation) -> TransactionRejection {
    TransactionRejection::DuplicateExternalId(cv.value().unwrap_or_default().to_owned())
}

fn id_taken(cv: TransactionConstraintViolation) -> TransactionRejection {
    TransactionRejection::DuplicateId(cv.value().unwrap_or_default().to_owned())
}

#[derive(Debug, Error)]
pub enum TransactionError {
    #[error(transparent)]
    Rejected(#[from] TransactionRejection),
    #[error(transparent)]
    Transient(#[from] errlanes::Transient),
    #[error(transparent)]
    Fatal(#[from] errlanes::Fatal),
}

impl_lane_error_fault!(TransactionError);
impl_lane_error_fail!(
    TransactionError,
    TransactionRejection,
    TransactionConstraintViolation
);
