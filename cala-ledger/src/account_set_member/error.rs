use es_entity::errlanes;

use crate::primitives::AccountSetId;

/// Initial account-set IDs that do not exist. SQL failures travel in the
/// surrounding carrier's fault lanes; the account service maps this rejection
/// to its public initial-membership outcome.
#[derive(errlanes::Rejection, Debug)]
pub(crate) enum AccountSetMemberRejection {
    #[error("AccountSetMemberRejection - AccountSetsNotFound: {0:?}")]
    #[rejection(code = "CALA_ACCOUNT_SET_MEMBER_ACCOUNT_SETS_NOT_FOUND")]
    AccountSetsNotFound(Vec<AccountSetId>),
}
