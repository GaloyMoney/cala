pub(crate) use super::repo::AccountConstraintViolation;
use crate::primitives::*;
use es_entity::errlanes;

#[derive(Debug, errlanes::Rejection)]
#[rejection(code = "CALA_ACCOUNT_CODE_ALREADY_EXISTS")]
#[error("CodeAlreadyExists: {0:?}")]
pub struct AccountCodeAlreadyExists(pub Option<String>);

impl From<es_entity::ConstraintConflict<String>> for AccountCodeAlreadyExists {
    fn from(conflict: es_entity::ConstraintConflict<String>) -> Self {
        Self(conflict.attempted)
    }
}

#[derive(Debug, errlanes::Rejection)]
#[rejection(code = "CALA_ACCOUNT_EXTERNAL_ID_ALREADY_EXISTS")]
#[error("ExternalIdAlreadyExists: {0:?}")]
pub struct AccountExternalIdAlreadyExists(pub Option<Option<String>>);

impl From<es_entity::ConstraintConflict<Option<String>>> for AccountExternalIdAlreadyExists {
    fn from(conflict: es_entity::ConstraintConflict<Option<String>>) -> Self {
        Self(conflict.attempted)
    }
}

#[derive(Debug, errlanes::Rejection, errlanes::Lift)]
#[lift(AccountConstraintViolation, unhandled = fatal)]
pub enum PersistAccountRejection {
    #[rejection(delegate, from)]
    #[error("{0}")]
    #[lift(AccountConstraintViolation::CodeKey, into)]
    CodeAlreadyExists(AccountCodeAlreadyExists),
    #[rejection(delegate, from)]
    #[error("{0}")]
    #[lift(AccountConstraintViolation::ExternalIdKey, into)]
    ExternalIdAlreadyExists(AccountExternalIdAlreadyExists),
    #[rejection(code = "CALA_ACCOUNT_CANNOT_UPDATE_ACCOUNT_SET_ACCOUNTS")]
    #[error("Cannot update accounts backing an account set")]
    CannotUpdateAccountSetAccounts,
}

#[derive(Debug, errlanes::Rejection)]
#[rejection(code = "CALA_ACCOUNT_COULD_NOT_FIND_BY_ID")]
#[error("Account not found: {0}")]
pub struct AccountNotFound(pub AccountId);

#[derive(Debug, errlanes::Rejection)]
#[rejection(code = "CALA_ACCOUNT_COULD_NOT_FIND_BY_CODE")]
#[error("Account code not found: {0}")]
pub struct AccountCodeNotFound(pub String);

#[derive(Debug, errlanes::Rejection)]
#[rejection(code = "CALA_ACCOUNT_COULD_NOT_FIND_BY_EXTERNAL_ID")]
#[error("Account external id not found: {0}")]
pub struct AccountExternalIdNotFound(pub String);

#[derive(Debug, errlanes::Rejection)]
#[rejection(code = "CALA_ACCOUNT_INITIAL_ACCOUNT_SET_NOT_FOUND")]
#[error("Initial account set not found: {0}")]
pub struct InitialAccountSetNotFound(pub AccountSetId);

#[derive(Debug, errlanes::Rejection, errlanes::Lift)]
#[lift(AccountConstraintViolation, unhandled = fatal)]
pub enum CreateAccountRejection {
    #[error("{0}")]
    #[rejection(delegate, from)]
    InitialAccountSetNotFound(InitialAccountSetNotFound),
    #[rejection(code = "CALA_ACCOUNT_DUPLICATE_ID")]
    #[error("DuplicateId: {0:?}")]
    #[lift(AccountConstraintViolation::Pkey, field = attempted)]
    DuplicateId(AccountId),
    #[rejection(delegate, from)]
    #[error("{0}")]
    #[lift(AccountConstraintViolation::CodeKey, into)]
    CodeAlreadyExists(AccountCodeAlreadyExists),
    #[rejection(delegate, from)]
    #[error("{0}")]
    #[lift(AccountConstraintViolation::ExternalIdKey, into)]
    ExternalIdAlreadyExists(AccountExternalIdAlreadyExists),
}

#[errlanes::compose(PersistAccountRejection)]
#[derive(Debug)]
pub enum SetAccountStatusRejection {
    #[error("{0}")]
    #[rejection(delegate, from)]
    AccountNotFound(AccountNotFound),
}
