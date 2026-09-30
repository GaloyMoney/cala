use thiserror::Error;

use cala_types::primitives::*;

#[errlanes::rejection]
#[derive(Debug, Clone, Error, errlanes::Lift)]
pub enum BalanceRejection {
    #[error("there is no balance recorded for journal {0}, account {1}, currency {2}")]
    NotFound(JournalId, AccountId, Currency),
    #[error("cannot update balances: the account {0} is locked")]
    AccountLocked(AccountId),
    #[flatten(prefix = "Journal")]
    Journal(crate::journal::error::JournalLookupRejection),
}

pub type BalanceError = errlanes::Fail<BalanceRejection, crate::CalaLanes>;

pub(super) fn corrupt_snapshot(
    context: impl Into<String>,
    e: serde_json::Error,
) -> crate::CalaFault {
    errlanes::Fatal::from_error(errlanes::FatalKind::CorruptState, e)
        .with_context(context.into())
        .into()
}
