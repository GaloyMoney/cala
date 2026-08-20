use thiserror::Error;

use crate::primitives::AccountSetId;

/// Error type for [`super::AccountSetMembers::attach_new_accounts_in_op`] —
/// the ONLY method on this module whose failure mode is a domain error
/// rather than a bare `sqlx::Error`. Every other method (locks, the classic
/// insert/remove, the member reads) propagates `sqlx::Error` directly:
/// callers already sit behind `AccountSetError` / `AccountError`, both of
/// which have a blanket `From<sqlx::Error>`, so a dedicated variant here
/// would be pure ceremony.
#[derive(Error, Debug)]
pub(crate) enum AccountSetMemberError {
    #[error("AccountSetMemberError - Sqlx: {0}")]
    Sqlx(#[from] sqlx::Error),
    #[error("AccountSetMemberError - AccountSetsNotFound: {0:?}")]
    AccountSetsNotFound(Vec<AccountSetId>),
}
