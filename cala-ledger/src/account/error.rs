use super::repo::AccountConstraintViolation;
use es_entity::errlanes;

use crate::primitives::{AccountId, AccountSetId};

#[derive(errlanes::Rejection, errlanes::Lift, Debug)]
#[lift(AccountConstraintViolation, unhandled = fatal)]
pub enum AccountRejection {
    #[error("duplicate id {0}")]
    #[rejection(code = "CALA_ACCOUNT_DUPLICATE_ID")]
    #[lift(AccountConstraintViolation::Pkey, field = attempted)]
    DuplicateId(AccountId),
    #[error("AccountRejection - NotFound: id '{0}' not found")]
    #[rejection(code = "CALA_ACCOUNT_COULD_NOT_FIND_BY_ID")]
    CouldNotFindById(AccountId),
    #[error("AccountRejection - NotFound: external id '{0}' not found")]
    #[rejection(code = "CALA_ACCOUNT_COULD_NOT_FIND_BY_EXTERNAL_ID")]
    CouldNotFindByExternalId(String),
    #[error("AccountRejection - NotFound: code '{0}' not found")]
    #[rejection(code = "CALA_ACCOUNT_COULD_NOT_FIND_BY_CODE")]
    CouldNotFindByCode(String),
    #[error("AccountRejection - external_id '{0:?}' already exists")]
    #[rejection(code = "CALA_ACCOUNT_EXTERNAL_ID_ALREADY_EXISTS")]
    #[lift(AccountConstraintViolation::ExternalIdKey, field = attempted)]
    ExternalIdAlreadyExists(Option<Option<String>>),
    #[error("AccountRejection - code '{0:?}' already exists")]
    #[rejection(code = "CALA_ACCOUNT_CODE_ALREADY_EXISTS")]
    #[lift(AccountConstraintViolation::CodeKey, field = attempted)]
    CodeAlreadyExists(Option<String>),
    #[error("AccountRejection - cannot update accounts backing an AccountSet")]
    #[rejection(code = "CALA_ACCOUNT_CANNOT_UPDATE_ACCOUNT_SET_ACCOUNTS")]
    CannotUpdateAccountSetAccounts,
    #[error("AccountRejection - initial account set '{0}' not found")]
    #[rejection(code = "CALA_ACCOUNT_INITIAL_ACCOUNT_SET_NOT_FOUND")]
    InitialAccountSetNotFound(AccountSetId),
}
