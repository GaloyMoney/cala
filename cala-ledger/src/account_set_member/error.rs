use thiserror::Error;

use crate::error_support::impl_lane_error_fault_only;
use crate::primitives::AccountSetId;

/// `cala_account_set_member_accounts` is a plain relation, not an `EsRepo`
/// entity — the "constraint fired at an existence check" case here is a
/// foreign key on a hand-written insert, so it becomes a rejection the same
/// way `account_set::AccountSetRejection::MemberAlreadyAdded` does: matched
/// on the exact constraint name in `From<sqlx::Error>`, never
/// `errlanes::Lift` (there is no generated `ConstraintViolation` for a
/// table this module doesn't own via `EsRepo`).
#[derive(Debug, Clone, Error, errlanes::Rejection)]
pub enum AccountSetMemberRejection {
    #[error("account set(s) not found: {0:?}")]
    AccountSetsNotFound(Vec<AccountSetId>),
}

/// Error type for [`super::AccountSetMembers::attach_new_accounts_in_op`] —
/// the ONLY method on this module whose failure mode is a domain error
/// rather than a bare `sqlx::Error`. Every other method (locks, the classic
/// insert/remove, the member reads) propagates `sqlx::Error` directly:
/// callers already sit behind `AccountSetError` / `AccountError`, both of
/// which have their own `From<sqlx::Error>`, so a dedicated variant here
/// would be pure ceremony.
#[derive(Debug, Error)]
pub(crate) enum AccountSetMemberError {
    #[error(transparent)]
    Rejected(#[from] AccountSetMemberRejection),
    #[error(transparent)]
    Transient(#[from] errlanes::Transient),
    #[error(transparent)]
    Fatal(#[from] errlanes::Fatal),
}

impl_lane_error_fault_only!(AccountSetMemberError);

impl From<sqlx::Error> for AccountSetMemberError {
    fn from(e: sqlx::Error) -> Self {
        errlanes::Fault::from(e).into()
    }
}
