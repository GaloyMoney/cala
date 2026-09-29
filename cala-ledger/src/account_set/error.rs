use thiserror::Error;

use crate::account::error::AccountRejection;
use crate::balance::error::BalanceRejection;
use crate::entry::error::EntryRejection;
use crate::error_support::{impl_lane_error_fail, impl_lane_error_fault_only};
use crate::primitives::{AccountId, AccountSetId};

use super::repo::{AccountSetConstraint, AccountSetConstraintViolation};

#[derive(Debug, Clone, Error, errlanes::Rejection)]
#[rejection(lift(AccountSetConstraintViolation))]
pub enum AccountSetRejection {
    #[error("account set '{0}' not found")]
    NotFoundById(AccountSetId),
    #[error("account set with external id '{0}' not found")]
    NotFoundByExternalId(String),
    #[error("external id '{0}' already exists")]
    #[rejection(key = AccountSetConstraint::ExternalIdKey, with = external_id_taken)]
    ExternalIdAlreadyExists(String),
    #[error("journal id mismatch")]
    JournalIdMismatch,
    /// Raised both from a client-side path-uniqueness check
    /// (`graph_validation`, `graph_cache`) and, as a defense-in-depth
    /// fallback, from the two hand-rolled member-edge tables' unique
    /// violations (`From<sqlx::Error>` below) — neither table is an
    /// `EsRepo` entity of its own, so there is no generated
    /// `ConstraintViolation` to lift through.
    #[error("member already added to account set")]
    MemberAlreadyAdded,
    #[error(
        "cannot add or remove member '{member_id}' to/from account set '{account_set_id}': \
         member already has balance history in this journal"
    )]
    MemberHasBalanceHistory {
        account_set_id: AccountSetId,
        member_id: AccountId,
    },
    #[error(
        "cannot add account set '{member_account_set_id}' as a member of account set \
         '{account_set_id}': the member is already an ancestor of the set, so the membership \
         would create a cycle"
    )]
    MembershipCycleDetected {
        account_set_id: AccountSetId,
        member_account_set_id: AccountSetId,
    },
    #[error(
        "cannot add account set '{member_account_set_id}' as a member of account set \
         '{account_set_id}': the resulting membership chain would be {depth} levels deep, \
         exceeding the maximum of {max}"
    )]
    MembershipDepthExceeded {
        account_set_id: AccountSetId,
        member_account_set_id: AccountSetId,
        depth: i32,
        max: i32,
    },
    #[error(transparent)]
    Account(#[from] AccountRejection),
    #[error(transparent)]
    Balance(#[from] BalanceRejection),
    #[error(transparent)]
    Entry(#[from] EntryRejection),
}

fn external_id_taken(cv: AccountSetConstraintViolation) -> AccountSetRejection {
    AccountSetRejection::ExternalIdAlreadyExists(cv.value().unwrap_or_default().to_owned())
}

#[derive(Debug, Error)]
pub enum AccountSetError {
    #[error(transparent)]
    Rejected(#[from] AccountSetRejection),
    #[error(transparent)]
    Transient(#[from] errlanes::Transient),
    #[error(transparent)]
    Fatal(#[from] errlanes::Fatal),
}

impl_lane_error_fault_only!(AccountSetError);
impl_lane_error_fail!(
    AccountSetError,
    AccountSetRejection,
    AccountSetConstraintViolation
);

/// The two member-edge tables (`cala_account_set_member_accounts`,
/// `cala_account_set_member_account_sets`) each carry exactly one unique
/// constraint guarding against a duplicate membership edge; neither table
/// is an `EsRepo` entity, so these can't come through the generated
/// `ConstraintViolation`/`Lift` path the way `ExternalIdAlreadyExists`
/// does. Both names are confirmed against the live schema — Postgres's
/// 63-byte identifier truncation makes them unguessable from the migration
/// source alone — and matched exactly, never by substring.
const ACCOUNT_MEMBER_UNIQUE_CONSTRAINT: &str =
    "cala_account_set_member_accou_member_account_id_account_set_key";
const SET_MEMBER_UNIQUE_CONSTRAINT: &str =
    "cala_account_set_member_accou_account_set_id_member_account_key";

impl From<sqlx::Error> for AccountSetError {
    fn from(e: sqlx::Error) -> Self {
        if let sqlx::Error::Database(ref db_err) = e {
            if let Some(constraint) = db_err.constraint() {
                if constraint == ACCOUNT_MEMBER_UNIQUE_CONSTRAINT
                    || constraint == SET_MEMBER_UNIQUE_CONSTRAINT
                {
                    return AccountSetRejection::MemberAlreadyAdded.into();
                }
            }
        }
        errlanes::Fault::from(e).into()
    }
}

impl From<crate::account::error::AccountError> for AccountSetError {
    fn from(e: crate::account::error::AccountError) -> Self {
        match e {
            crate::account::error::AccountError::Rejected(r) => {
                Self::Rejected(AccountSetRejection::Account(r))
            }
            crate::account::error::AccountError::Transient(t) => Self::Transient(t),
            crate::account::error::AccountError::Fatal(f) => Self::Fatal(f),
        }
    }
}

impl From<crate::balance::error::BalanceError> for AccountSetError {
    fn from(e: crate::balance::error::BalanceError) -> Self {
        match e {
            crate::balance::error::BalanceError::Rejected(r) => {
                Self::Rejected(AccountSetRejection::Balance(r))
            }
            crate::balance::error::BalanceError::Transient(t) => Self::Transient(t),
            crate::balance::error::BalanceError::Fatal(f) => Self::Fatal(f),
        }
    }
}

impl From<crate::entry::error::EntryError> for AccountSetError {
    fn from(e: crate::entry::error::EntryError) -> Self {
        match e {
            crate::entry::error::EntryError::Rejected(r) => {
                Self::Rejected(AccountSetRejection::Entry(r))
            }
            crate::entry::error::EntryError::Transient(t) => Self::Transient(t),
            crate::entry::error::EntryError::Fatal(f) => Self::Fatal(f),
        }
    }
}
