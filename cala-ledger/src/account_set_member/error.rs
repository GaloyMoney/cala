use crate::primitives::AccountSetId;
use es_entity::errlanes;
#[derive(Debug, errlanes::Rejection)]
#[rejection(code = "CALA_ACCOUNT_SET_MEMBER_ACCOUNT_SETS_NOT_FOUND")]
#[error("Initial account sets not found: {0:?}")]
pub struct InitialAccountSetsNotFound(pub Vec<AccountSetId>);
