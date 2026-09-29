use thiserror::Error;

use cala_types::primitives::*;

use crate::error_support::impl_lane_error_fault;
use crate::journal::error::JournalRejection;

#[derive(Debug, Clone, Error, errlanes::Rejection)]
pub enum BalanceRejection {
    #[error("there is no balance recorded for journal {0}, account {1}, currency {2}")]
    NotFound(JournalId, AccountId, Currency),
    #[error("cannot update balances: the account {0} is locked")]
    AccountLocked(AccountId),
    #[error(transparent)]
    Journal(#[from] JournalRejection),
}

#[derive(Debug, Error)]
pub enum BalanceError {
    #[error(transparent)]
    Rejected(#[from] BalanceRejection),
    #[error(transparent)]
    Transient(#[from] errlanes::Transient),
    #[error(transparent)]
    Fatal(#[from] errlanes::Fatal),
}

impl_lane_error_fault!(BalanceError);

/// A `serde_json`-stored balance snapshot that no longer deserializes is
/// corrupt state, not a caller-correctable outcome — a decode failure here
/// always traces back to something outside cala having written or migrated
/// the JSONB column, never to caller input. `context` names the row so an
/// operator paged on this has somewhere to start looking.
pub(super) fn corrupt_snapshot(context: impl Into<String>, e: serde_json::Error) -> BalanceError {
    BalanceError::Fatal(
        errlanes::Fatal::from_error(errlanes::FatalKind::CorruptState, e)
            .with_context(context.into()),
    )
}

impl From<crate::journal::error::JournalError> for BalanceError {
    fn from(e: crate::journal::error::JournalError) -> Self {
        match e {
            crate::journal::error::JournalError::Rejected(r) => {
                Self::Rejected(BalanceRejection::Journal(r))
            }
            crate::journal::error::JournalError::Transient(t) => Self::Transient(t),
            crate::journal::error::JournalError::Fatal(f) => Self::Fatal(f),
        }
    }
}
