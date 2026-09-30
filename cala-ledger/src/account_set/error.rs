use thiserror::Error;

use crate::primitives::{AccountId, AccountSetId};

use super::repo::AccountSetConstraintViolation;

#[errlanes::compose]
#[derive(Debug, Clone, Error)]
#[lift(AccountSetConstraintViolation, unhandled = fatal)]
pub enum AccountSetRejection {
    #[error("account set '{0}' not found")]
    NotFoundById(AccountSetId),
    #[error("account set with external id '{0}' not found")]
    NotFoundByExternalId(String),
    #[error("external id '{0}' already exists")]
    #[lift(AccountSetConstraintViolation::ExternalIdKey)]
    #[rejection(code = "EXTERNAL_ID_ALREADY_EXISTS")]
    ExternalIdAlreadyExists(#[source] es_entity::ConstraintConflict<Option<String>>),
    #[error("journal id mismatch")]
    JournalIdMismatch,
    /// Raised both from a client-side path-uniqueness check
    /// (`graph_validation`, `graph_cache`) and, as a defense-in-depth
    /// fallback, from the two hand-rolled member-edge tables' unique
    /// violations (classified at the insert boundary) — neither table is an
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
    #[compose(flatten)]
    Account(crate::account::error::AccountRejection),
}

pub type AccountSetError = errlanes::Fail<AccountSetRejection, crate::CalaLanes>;

/// Only the two membership-edge inserts interpret these exact unique constraints.
pub(crate) fn membership_write_error(error: sqlx::Error) -> AccountSetError {
    if let sqlx::Error::Database(db) = &error {
        if db.is_unique_violation()
            && matches!(
                db.constraint(),
                Some("cala_account_set_member_accou_member_account_id_account_set_key")
                    | Some("cala_account_set_member_accou_account_set_id_member_account_key")
            )
        {
            return AccountSetRejection::MemberAlreadyAdded.into();
        }
    }
    error.into()
}
