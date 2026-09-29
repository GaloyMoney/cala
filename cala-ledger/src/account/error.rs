use thiserror::Error;

use crate::error_support::{impl_lane_error_fail, impl_lane_error_fault};

use super::repo::{AccountConstraint, AccountConstraintViolation};
use crate::primitives::{AccountId, AccountSetId};

#[derive(Debug, Clone, Error, errlanes::Rejection)]
#[rejection(lift(AccountConstraintViolation))]
pub enum AccountRejection {
    #[error("account '{0}' not found")]
    NotFoundById(AccountId),
    #[error("account with external id '{0}' not found")]
    NotFoundByExternalId(String),
    #[error("account with code '{0}' not found")]
    NotFoundByCode(String),
    #[error("external id '{0}' already exists")]
    #[rejection(key = AccountConstraint::ExternalIdKey, with = external_id_taken)]
    ExternalIdAlreadyExists(String),
    #[error("code '{0}' already exists")]
    #[rejection(key = AccountConstraint::CodeKey, with = code_taken)]
    CodeAlreadyExists(String),
    #[error("cannot update accounts backing an AccountSet")]
    CannotUpdateAccountSetAccounts,
    #[error("initial account set '{0}' not found")]
    InitialAccountSetNotFound(AccountSetId),
}

fn external_id_taken(cv: AccountConstraintViolation) -> AccountRejection {
    AccountRejection::ExternalIdAlreadyExists(cv.value().unwrap_or_default().to_owned())
}

fn code_taken(cv: AccountConstraintViolation) -> AccountRejection {
    AccountRejection::CodeAlreadyExists(cv.value().unwrap_or_default().to_owned())
}

#[derive(Debug, Error)]
pub enum AccountError {
    #[error(transparent)]
    Rejected(#[from] AccountRejection),
    #[error(transparent)]
    Transient(#[from] errlanes::Transient),
    #[error(transparent)]
    Fatal(#[from] errlanes::Fatal),
}

impl_lane_error_fault!(AccountError);
impl_lane_error_fail!(AccountError, AccountRejection, AccountConstraintViolation);
