pub(crate) use super::repo::AccountSetConstraintViolation;
use crate::primitives::*;
use es_entity::errlanes;

#[derive(Debug, errlanes::Rejection)]
#[rejection(code = "CALA_ACCOUNT_SET_EXTERNAL_ID_ALREADY_EXISTS")]
#[error("Account set external id already exists: {0:?}")]
pub struct AccountSetExternalIdAlreadyExists(pub Option<Option<String>>);

impl From<es_entity::ConstraintConflict<Option<String>>> for AccountSetExternalIdAlreadyExists {
    fn from(conflict: es_entity::ConstraintConflict<Option<String>>) -> Self {
        Self(conflict.attempted)
    }
}

#[derive(Debug, errlanes::Rejection)]
#[rejection(code = "CALA_ACCOUNT_SET_JOURNAL_ID_MISMATCH")]
#[error("Account sets must belong to the same journal")]
pub struct AccountSetJournalIdMismatch;

#[derive(Debug, errlanes::Rejection)]
#[rejection(code = "CALA_ACCOUNT_SET_COULD_NOT_FIND_BY_ID")]
#[error("Account set not found: {0}")]
pub struct AccountSetNotFound(pub AccountSetId);

#[derive(Debug, errlanes::Rejection)]
#[rejection(code = "CALA_ACCOUNT_SET_COULD_NOT_FIND_BY_EXTERNAL_ID")]
#[error("Account set external id not found: {0}")]
pub struct AccountSetExternalIdNotFound(pub String);

#[derive(Debug, errlanes::Rejection, errlanes::Lift)]
#[lift(AccountSetConstraintViolation, unhandled = fatal)]
pub enum PersistAccountSetRejection {
    #[error("{0}")]
    #[rejection(delegate, from)]
    #[lift(AccountSetConstraintViolation::ExternalIdKey, into)]
    ExternalIdAlreadyExists(AccountSetExternalIdAlreadyExists),
}

#[derive(Debug, errlanes::Rejection, errlanes::Lift)]
// The set row is inserted only after its backing account in the same op.
// Account Pkey/CodeKey conflicts reject; a later set Pkey conflict is fatal.
#[lift(crate::account::error::AccountConstraintViolation, unhandled = fatal)]
#[lift(AccountSetConstraintViolation, unhandled = fatal)]
pub enum CreateAccountSetRejection {
    #[error("Backing account id already exists: {0}")]
    #[rejection(code = "CALA_ACCOUNT_SET_BACKING_ACCOUNT_ID_ALREADY_EXISTS")]
    #[lift(crate::account::error::AccountConstraintViolation::Pkey, field = attempted)]
    BackingDuplicateId(AccountId),
    #[error("Backing account code already exists: {0:?}")]
    #[rejection(code = "CALA_ACCOUNT_SET_BACKING_ACCOUNT_CODE_ALREADY_EXISTS")]
    #[lift(crate::account::error::AccountConstraintViolation::CodeKey, field = attempted)]
    BackingCodeAlreadyExists(Option<String>),
    #[error("{0}")]
    #[rejection(delegate, from)]
    #[lift(AccountSetConstraintViolation::ExternalIdKey, into)]
    SetExternalIdAlreadyExists(AccountSetExternalIdAlreadyExists),
}

#[derive(Debug, errlanes::Rejection)]
#[rejection(code = "CALA_ACCOUNT_SET_MEMBER_ALREADY_ADDED")]
#[error("Member already added to account set")]
pub struct MemberAlreadyAdded;

#[derive(Debug, errlanes::Rejection)]
#[rejection(code = "CALA_ACCOUNT_SET_MEMBER_HAS_BALANCE_HISTORY")]
#[error(
    "Member {} has balance history in account set {}'s journal",
    member_id,
    account_set_id
)]
pub struct MemberHasBalanceHistory {
    pub account_set_id: AccountSetId,
    pub member_id: AccountId,
}

#[derive(Debug, errlanes::Rejection)]
pub enum AddAccountMembersRejection {
    #[error("{0}")]
    #[rejection(delegate, from)]
    AccountSetNotFound(AccountSetNotFound),
    #[error("{0}")]
    #[rejection(delegate, from)]
    MemberHasBalanceHistory(MemberHasBalanceHistory),
    #[error("{0}")]
    #[rejection(delegate, from)]
    MemberAlreadyAdded(MemberAlreadyAdded),
}

#[derive(Debug, errlanes::Rejection)]
pub enum AddSetMembersRejection {
    #[error("{0}")]
    #[rejection(delegate, from)]
    AccountSetNotFound(AccountSetNotFound),
    #[error("{0}")]
    #[rejection(delegate, from)]
    MemberHasBalanceHistory(MemberHasBalanceHistory),
    #[rejection(delegate, from)]
    #[error("{0}")]
    JournalIdMismatch(AccountSetJournalIdMismatch),
    #[error("{0}")]
    #[rejection(delegate, from)]
    MemberAlreadyAdded(MemberAlreadyAdded),
    #[rejection(code = "CALA_ACCOUNT_SET_MEMBERSHIP_CYCLE_DETECTED")]
    #[error(
        "Membership {} -> {} would create a cycle",
        account_set_id,
        member_account_set_id
    )]
    MembershipCycleDetected {
        account_set_id: AccountSetId,
        member_account_set_id: AccountSetId,
    },
    #[rejection(code = "CALA_ACCOUNT_SET_MEMBERSHIP_DEPTH_EXCEEDED")]
    #[error(
        "Membership {} -> {} exceeds maximum depth {}: {}",
        account_set_id,
        member_account_set_id,
        max,
        depth
    )]
    MembershipDepthExceeded {
        account_set_id: AccountSetId,
        member_account_set_id: AccountSetId,
        depth: i32,
        max: i32,
    },
}

#[errlanes::compose(AddAccountMembersRejection, AddSetMembersRejection)]
#[derive(Debug)]
pub enum AddMemberRejection {
    #[compose(merge)]
    #[error("{0}")]
    #[rejection(delegate, from)]
    AccountSetNotFound(AccountSetNotFound),
    #[compose(merge)]
    #[error("{0}")]
    #[rejection(delegate, from)]
    MemberHasBalanceHistory(MemberHasBalanceHistory),
    #[compose(merge)]
    #[error("{0}")]
    #[rejection(delegate, from)]
    MemberAlreadyAdded(MemberAlreadyAdded),
}

#[derive(Debug, errlanes::Rejection)]
pub enum RemoveMemberRejection {
    #[error("{0}")]
    #[rejection(delegate, from)]
    AccountSetNotFound(AccountSetNotFound),
    #[error("{0}")]
    #[rejection(delegate, from)]
    MemberHasBalanceHistory(MemberHasBalanceHistory),
    #[rejection(delegate, from)]
    #[error("{0}")]
    JournalIdMismatch(AccountSetJournalIdMismatch),
}

/// Classifies this write's known constraints; every other SQL failure keeps its native lane.
#[derive(Debug, errlanes::Classify)]
pub(crate) enum MembershipWrite {
    #[classify(delegate)]
    Domain(MemberAlreadyAdded),
    #[classify(delegate)]
    Sqlx(sqlx::Error),
}
impl From<sqlx::Error> for MembershipWrite {
    fn from(error: sqlx::Error) -> Self {
        match error.as_database_error().and_then(|e| e.constraint()) {
            Some("cala_account_set_member_accou_account_set_id_member_account_key") => {
                Self::Domain(MemberAlreadyAdded)
            }
            Some("cala_account_set_member_accou_account_set_id_member_accoun_key1") => {
                Self::Domain(MemberAlreadyAdded)
            }
            _ => Self::Sqlx(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use es_entity::errlanes::{lanes, Fail, FatalKind, ResultExt};
    use std::error::Error;

    #[test]
    fn set_row_primary_key_after_backing_insert_is_an_invariant() {
        let constraint = AccountSetConstraintViolation::pkey_from_database(
            sqlx::Error::Protocol("unexpected second insert collision".into()),
            AccountSetId::new(),
        );
        let result: Result<(), Fail<CreateAccountSetRejection, lanes!(Transient, Fatal)>> =
            Err::<(), _>(constraint).widen();
        let Fail::Fatal(fault) = result.unwrap_err() else {
            panic!("must be an invariant fault")
        };
        assert_eq!(fault.kind, FatalKind::Invariant);
        assert!(fault
            .source()
            .unwrap()
            .is::<AccountSetConstraintViolation>());
    }
}
