use thiserror::Error;

use super::repo::AccountConstraintViolation;
use crate::primitives::{AccountId, AccountSetId};

#[derive(Debug, Clone, Error, errlanes::Rejection, errlanes::Lift)]
#[lift(AccountConstraintViolation, unhandled = fatal)]
pub enum AccountRejection {
    #[error("account '{0}' not found")]
    NotFoundById(AccountId),
    #[error("account with external id '{0}' not found")]
    NotFoundByExternalId(String),
    #[error("account with code '{0}' not found")]
    NotFoundByCode(String),
    #[error("external id '{0}' already exists")]
    #[lift(AccountConstraintViolation::ExternalIdKey)]
    #[rejection(code = "EXTERNAL_ID_ALREADY_EXISTS")]
    ExternalIdAlreadyExists(#[source] es_entity::ConstraintConflict<Option<String>>),
    #[error("code '{0}' already exists")]
    #[lift(AccountConstraintViolation::CodeKey)]
    #[rejection(code = "CODE_ALREADY_EXISTS")]
    CodeAlreadyExists(#[source] es_entity::ConstraintConflict<String>),
    #[error("cannot update accounts backing an AccountSet")]
    CannotUpdateAccountSetAccounts,
    #[error("initial account set '{0}' not found")]
    InitialAccountSetNotFound(AccountSetId),
}

pub type AccountError = errlanes::Fail<AccountRejection, crate::CalaLanes>;
