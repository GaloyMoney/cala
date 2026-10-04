use super::repo::AccountConstraintViolation;
use crate::primitives::*;
use es_entity::errlanes;

#[derive(Debug, errlanes::Rejection, errlanes::Lift)]
#[lift(AccountConstraintViolation, unhandled = fatal)]
pub enum InsertAccountRejection {
    #[rejection(code = "CALA_ACCOUNT_DUPLICATE_ID")]
    #[error("DuplicateId: {0:?}")]
    #[lift(AccountConstraintViolation::Pkey, field = attempted)]
    DuplicateId(AccountId),
    #[rejection(code = "CALA_ACCOUNT_CODE_ALREADY_EXISTS")]
    #[error("CodeAlreadyExists: {0:?}")]
    #[lift(AccountConstraintViolation::CodeKey, field = attempted)]
    CodeAlreadyExists(Option<String>),
    #[rejection(code = "CALA_ACCOUNT_EXTERNAL_ID_ALREADY_EXISTS")]
    #[error("ExternalIdAlreadyExists: {0:?}")]
    #[lift(AccountConstraintViolation::ExternalIdKey, field = attempted)]
    ExternalIdAlreadyExists(Option<Option<String>>),
}

#[derive(Debug, errlanes::Rejection, errlanes::Lift)]
#[lift(AccountConstraintViolation, unhandled = fatal)]
pub enum PersistAccountRejection {
    #[rejection(code = "CALA_ACCOUNT_CODE_ALREADY_EXISTS")]
    #[error("CodeAlreadyExists: {0:?}")]
    #[lift(AccountConstraintViolation::CodeKey, field = attempted)]
    CodeAlreadyExists(Option<String>),
    #[rejection(code = "CALA_ACCOUNT_EXTERNAL_ID_ALREADY_EXISTS")]
    #[error("ExternalIdAlreadyExists: {0:?}")]
    #[lift(AccountConstraintViolation::ExternalIdKey, field = attempted)]
    ExternalIdAlreadyExists(Option<Option<String>>),
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

impl InitialAccountSetNotFound {
    pub(crate) fn from_missing(
        missing: crate::account_set_member::InitialAccountSetsNotFound,
    ) -> Self {
        Self(*missing.0.first().expect("missing ids are never empty"))
    }
}

#[derive(Debug, errlanes::Rejection, errlanes::Lift)]
#[lift(InsertAccountRejection)]
pub enum CreateAccountRejection {
    #[rejection(code = "CALA_ACCOUNT_INITIAL_ACCOUNT_SET_NOT_FOUND")]
    #[error("{0}")]
    #[rejection(from)]
    InitialAccountSetNotFound(InitialAccountSetNotFound),
    #[rejection(code = "CALA_ACCOUNT_DUPLICATE_ID")]
    #[error("DuplicateId: {0:?}")]
    #[lift(InsertAccountRejection::DuplicateId)]
    DuplicateId(AccountId),
    #[rejection(code = "CALA_ACCOUNT_CODE_ALREADY_EXISTS")]
    #[error("CodeAlreadyExists: {0:?}")]
    #[lift(InsertAccountRejection::CodeAlreadyExists)]
    CodeAlreadyExists(Option<String>),
    #[rejection(code = "CALA_ACCOUNT_EXTERNAL_ID_ALREADY_EXISTS")]
    #[error("ExternalIdAlreadyExists: {0:?}")]
    #[lift(InsertAccountRejection::ExternalIdAlreadyExists)]
    ExternalIdAlreadyExists(Option<Option<String>>),
}

#[derive(Debug, errlanes::Rejection, errlanes::Lift)]
#[lift(PersistAccountRejection)]
pub enum SetAccountStatusRejection {
    #[rejection(code = "CALA_ACCOUNT_COULD_NOT_FIND_BY_ID")]
    #[error("{0}")]
    #[rejection(from)]
    AccountNotFound(AccountNotFound),
    #[rejection(code = "CALA_ACCOUNT_CODE_ALREADY_EXISTS")]
    #[error("CodeAlreadyExists: {0:?}")]
    #[lift(PersistAccountRejection::CodeAlreadyExists)]
    CodeAlreadyExists(Option<String>),
    #[rejection(code = "CALA_ACCOUNT_EXTERNAL_ID_ALREADY_EXISTS")]
    #[error("ExternalIdAlreadyExists: {0:?}")]
    #[lift(PersistAccountRejection::ExternalIdAlreadyExists)]
    ExternalIdAlreadyExists(Option<Option<String>>),
    #[rejection(code = "CALA_ACCOUNT_CANNOT_UPDATE_ACCOUNT_SET_ACCOUNTS")]
    #[error("Cannot update accounts backing an account set")]
    #[lift(PersistAccountRejection::CannotUpdateAccountSetAccounts)]
    CannotUpdateAccountSetAccounts,
}

#[derive(Debug, errlanes::Rejection, errlanes::Lift)]
#[lift(AccountConstraintViolation, unhandled = fatal)]
pub enum CreateBackingAccountRejection {
    #[rejection(code = "CALA_ACCOUNT_DUPLICATE_ID")]
    #[error("DuplicateId: {0:?}")]
    #[lift(AccountConstraintViolation::Pkey, field = attempted)]
    DuplicateId(AccountId),
    #[rejection(code = "CALA_ACCOUNT_CODE_ALREADY_EXISTS")]
    #[error("CodeAlreadyExists: {0:?}")]
    #[lift(AccountConstraintViolation::CodeKey, field = attempted)]
    CodeAlreadyExists(Option<String>),
}
