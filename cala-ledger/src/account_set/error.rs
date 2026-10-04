use super::repo::AccountSetConstraintViolation;
use es_entity::errlanes;

use crate::primitives::{AccountId, AccountSetId};

#[derive(errlanes::Rejection, errlanes::Lift, Debug)]
#[lift(AccountSetConstraintViolation, unhandled = fatal)]
pub enum AccountSetRejection {
    #[error("duplicate id {0}")]
    #[rejection(code = "CALA_ACCOUNTSET_DUPLICATE_ID")]
    #[lift(AccountSetConstraintViolation::Pkey, field = attempted)]
    DuplicateId(AccountSetId),
    #[error("AccountSetRejection - AccountRejection: {0}")]
    #[rejection(delegate, from)]
    AccountRejection(crate::account::error::AccountRejection),
    #[error("AccountSetRejection - NotFound: id '{0}' not found")]
    #[rejection(code = "CALA_ACCOUNT_SET_COULD_NOT_FIND_BY_ID")]
    CouldNotFindById(AccountSetId),
    #[error("AccountSetRejection - NotFound: external id '{0}' not found")]
    #[rejection(code = "CALA_ACCOUNT_SET_COULD_NOT_FIND_BY_EXTERNAL_ID")]
    CouldNotFindByExternalId(String),
    #[error("AccountSetRejection - external_id '{0:?}' already exists")]
    #[rejection(code = "CALA_ACCOUNT_SET_EXTERNAL_ID_ALREADY_EXISTS")]
    #[lift(AccountSetConstraintViolation::ExternalIdKey, field = attempted)]
    ExternalIdAlreadyExists(Option<Option<String>>),
    #[error("AccountSetRejection - JournalIdMismatch")]
    #[rejection(code = "CALA_ACCOUNT_SET_JOURNAL_ID_MISMATCH")]
    JournalIdMismatch,
    #[error("AccountSetRejection - Member already added to account set")]
    #[rejection(code = "CALA_ACCOUNT_SET_MEMBER_ALREADY_ADDED")]
    MemberAlreadyAdded,
    #[error(
        "AccountSetRejection - Cannot add or remove member '{member_id}' to/from \
         account set '{account_set_id}': member already has balance history \
         in this journal"
    )]
    #[rejection(code = "CALA_ACCOUNT_SET_MEMBER_HAS_BALANCE_HISTORY")]
    MemberHasBalanceHistory {
        account_set_id: AccountSetId,
        member_id: AccountId,
    },
    #[error(
        "AccountSetRejection - Cannot add account set '{member_account_set_id}' as a member of \
         account set '{account_set_id}': the member is already an ancestor of the set, \
         so the membership would create a cycle"
    )]
    #[rejection(code = "CALA_ACCOUNT_SET_MEMBERSHIP_CYCLE_DETECTED")]
    MembershipCycleDetected {
        account_set_id: AccountSetId,
        member_account_set_id: AccountSetId,
    },
    #[error(
        "AccountSetRejection - Cannot add account set '{member_account_set_id}' as a member of \
         account set '{account_set_id}': the resulting membership chain would be {depth} \
         levels deep, exceeding the maximum of {max}"
    )]
    #[rejection(code = "CALA_ACCOUNT_SET_MEMBERSHIP_DEPTH_EXCEEDED")]
    MembershipDepthExceeded {
        account_set_id: AccountSetId,
        member_account_set_id: AccountSetId,
        depth: i32,
        max: i32,
    },
}

/// Classifies this write's known constraints; every other SQL failure keeps its native lane.
#[derive(Debug, errlanes::Classify)]
pub(crate) enum MembershipWrite {
    #[classify(delegate)]
    Domain(AccountSetRejection),
    #[classify(delegate)]
    Sqlx(sqlx::Error),
}
impl From<sqlx::Error> for MembershipWrite {
    fn from(error: sqlx::Error) -> Self {
        match error.as_database_error().and_then(|e| e.constraint()) {
            Some("cala_account_set_member_accou_account_set_id_member_account_key") => {
                Self::Domain(AccountSetRejection::MemberAlreadyAdded)
            }
            Some("cala_account_set_member_accou_account_set_id_member_accoun_key1") => {
                Self::Domain(AccountSetRejection::MemberAlreadyAdded)
            }
            _ => Self::Sqlx(error),
        }
    }
}
