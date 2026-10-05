use thiserror::Error;

use crate::primitives::AccountSetId;

/// Missing sets reported by initial account attachment.
#[derive(Debug, Clone, Error, errlanes::Rejection)]
pub enum AccountSetMemberRejection {
    #[error("account set(s) not found: {0:?}")]
    AccountSetsNotFound(Vec<AccountSetId>),
}

/// Initial account attachment can reject; the other member operations return SQLx errors.
pub(crate) type AccountSetMemberError = errlanes::Fail<AccountSetMemberRejection, crate::CalaLanes>;
