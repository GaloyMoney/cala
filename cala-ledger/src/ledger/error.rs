use thiserror::Error;

use crate::{
    account::error::{AccountError, AccountRejection},
    account_set::error::{AccountSetError, AccountSetRejection},
    balance::error::{BalanceError, BalanceRejection},
    entry::error::{EntryError, EntryRejection},
    error_support::impl_lane_error_fault_only,
    journal::error::{JournalError, JournalRejection},
    posting::{PostingError, PostingRejection},
    transaction::error::{TransactionError, TransactionRejection},
    tx_template::error::{TxTemplateError, TxTemplateRejection},
    velocity::error::{VelocityError, VelocityRejection},
};

/// The ledger's top-level rejection — every module's own rejection, nested
/// one variant per module, plus the one rejection the ledger raises itself
/// ([`Self::EntryTargetsAccountSet`], hoisted out of [`EntryRejection`] so
/// callers do not have to dig through the entry-error nesting for the
/// single-most-common posting mistake).
///
/// A consumer that only cares about one module's outcomes matches exactly
/// one level deep, e.g. `LedgerRejection::Velocity(VelocityRejection::Enforcement(_))`.
#[derive(Debug, Error)]
pub enum LedgerRejection {
    #[error(
        "an entry may not be posted directly to an account-set backing account; \
         an account set's balance is derived from its members"
    )]
    EntryTargetsAccountSet,
    #[error(transparent)]
    Account(#[from] AccountRejection),
    #[error(transparent)]
    AccountSet(#[from] AccountSetRejection),
    #[error(transparent)]
    Journal(#[from] JournalRejection),
    #[error(transparent)]
    TxTemplate(#[from] TxTemplateRejection),
    #[error(transparent)]
    Transaction(#[from] TransactionRejection),
    #[error(transparent)]
    Entry(#[from] EntryRejection),
    #[error(transparent)]
    Balance(#[from] BalanceRejection),
    #[error(transparent)]
    Velocity(#[from] VelocityRejection),
    #[error(transparent)]
    Posting(#[from] PostingRejection),
}

#[derive(Debug, Error)]
pub enum LedgerError {
    #[error(transparent)]
    Rejected(#[from] LedgerRejection),
    #[error(transparent)]
    Transient(#[from] errlanes::Transient),
    #[error(transparent)]
    Fatal(#[from] errlanes::Fatal),
}

impl_lane_error_fault_only!(LedgerError);

/// `cala_transactions`'s primary key is the only constraint the ledger's own
/// hand-written SQL treats as a rejection; every other module already
/// classifies its own `sqlx::Error`s before an error ever reaches here.
const TRANSACTIONS_PKEY_CONSTRAINT: &str = "cala_transactions_pkey";

impl From<sqlx::Error> for LedgerError {
    fn from(e: sqlx::Error) -> Self {
        if let sqlx::Error::Database(ref db_err) = e {
            if db_err.constraint() == Some(TRANSACTIONS_PKEY_CONSTRAINT) {
                return Self::Rejected(LedgerRejection::Posting(
                    PostingRejection::DuplicateTransactionId,
                ));
            }
        }
        errlanes::Fault::from(e).into()
    }
}

impl From<sqlx::migrate::MigrateError> for LedgerError {
    fn from(e: sqlx::migrate::MigrateError) -> Self {
        Self::Fatal(errlanes::Fatal::from_error(errlanes::FatalKind::Config, e))
    }
}

/// A missing/malformed connection configuration is a misconfiguration, not
/// a caller-correctable outcome at ledger init time — nobody is retrying
/// their way out of "you must set `pg_con` or `pool`".
pub(super) fn config_error(message: impl Into<String>) -> LedgerError {
    LedgerError::Fatal(
        errlanes::Fatal::new(errlanes::FatalKind::Config).with_context(message.into()),
    )
}

impl From<Box<dyn std::error::Error + Send + Sync>> for LedgerError {
    fn from(e: Box<dyn std::error::Error + Send + Sync>) -> Self {
        Self::Fatal(errlanes::Fatal::from_boxed(
            errlanes::FatalKind::Dependency,
            e,
        ))
    }
}

/// Not a `From` on `EntryRejection` for `LedgerRejection` alone: only
/// `EntryTargetsAccountSet` is hoisted to the top level (the single most
/// common posting mistake); every other entry rejection stays nested under
/// [`LedgerRejection::Entry`].
impl From<EntryError> for LedgerError {
    fn from(e: EntryError) -> Self {
        match e {
            EntryError::Rejected(EntryRejection::EntryTargetsAccountSet) => {
                Self::Rejected(LedgerRejection::EntryTargetsAccountSet)
            }
            EntryError::Transient(t) => Self::Transient(t),
            EntryError::Fatal(f) => Self::Fatal(f),
        }
    }
}

/// The detail behind an `EcCaughtUpTimeout` — a wedged or slow streaming EC
/// rollup. Carried as the `Transient`'s `source` (exactly the way
/// `es_entity::NotFound` rides a `Fatal`'s source — see
/// `errlanes::Transient::source_arc`), so a caller that needs the observed
/// checkpoint/frontier/wait for an alert can downcast it rather than parse
/// the message.
#[derive(Debug, Error)]
#[error(
    "EC rollup checkpoint {applied} had not reached the outbox frontier {frontier} \
     after waiting {waited:?}"
)]
pub struct EcCaughtUpTimeout {
    pub applied: obix::StreamPosition,
    pub frontier: obix::StreamPosition,
    pub waited: std::time::Duration,
}

/// Manual, not `#[from]`: obix's `SubscriptionError` is not itself
/// lane-shaped (obix's own rollout section only bumped its dependency, it
/// did not reshape its errors), so this classifies by hand instead of
/// nesting. The timeout case is the one outcome a ledger caller can act on
/// (retry after waiting) — every other variant (a decode failure, a missing
/// handler job, ...) is this dependency being broken in a way no caller can
/// fix, so it demotes to `Fatal(Dependency)`.
impl From<obix::out::SubscriptionError> for LedgerError {
    fn from(e: obix::out::SubscriptionError) -> Self {
        match e {
            obix::out::SubscriptionError::CaughtUpTimeout {
                checkpoint,
                target,
                waited,
            } => {
                let detail = EcCaughtUpTimeout {
                    applied: checkpoint,
                    frontier: target,
                    waited,
                };
                Self::Transient(
                    errlanes::Transient::new(errlanes::TransientKind::Congestion)
                        .with_context(detail.to_string())
                        .with_source(detail),
                )
            }
            other => Self::Fatal(errlanes::Fatal::from_error(
                errlanes::FatalKind::Dependency,
                other,
            )),
        }
    }
}

impl From<AccountError> for LedgerError {
    fn from(e: AccountError) -> Self {
        match e {
            AccountError::Rejected(r) => Self::Rejected(LedgerRejection::Account(r)),
            AccountError::Transient(t) => Self::Transient(t),
            AccountError::Fatal(f) => Self::Fatal(f),
        }
    }
}

impl From<AccountSetError> for LedgerError {
    fn from(e: AccountSetError) -> Self {
        match e {
            AccountSetError::Rejected(r) => Self::Rejected(LedgerRejection::AccountSet(r)),
            AccountSetError::Transient(t) => Self::Transient(t),
            AccountSetError::Fatal(f) => Self::Fatal(f),
        }
    }
}

impl From<JournalError> for LedgerError {
    fn from(e: JournalError) -> Self {
        match e {
            JournalError::Rejected(r) => Self::Rejected(LedgerRejection::Journal(r)),
            JournalError::Transient(t) => Self::Transient(t),
            JournalError::Fatal(f) => Self::Fatal(f),
        }
    }
}

impl From<TxTemplateError> for LedgerError {
    fn from(e: TxTemplateError) -> Self {
        match e {
            TxTemplateError::Rejected(r) => Self::Rejected(LedgerRejection::TxTemplate(r)),
            TxTemplateError::Transient(t) => Self::Transient(t),
            TxTemplateError::Fatal(f) => Self::Fatal(f),
        }
    }
}

impl From<TransactionError> for LedgerError {
    fn from(e: TransactionError) -> Self {
        match e {
            TransactionError::Rejected(r) => Self::Rejected(LedgerRejection::Transaction(r)),
            TransactionError::Transient(t) => Self::Transient(t),
            TransactionError::Fatal(f) => Self::Fatal(f),
        }
    }
}

impl From<BalanceError> for LedgerError {
    fn from(e: BalanceError) -> Self {
        match e {
            BalanceError::Rejected(r) => Self::Rejected(LedgerRejection::Balance(r)),
            BalanceError::Transient(t) => Self::Transient(t),
            BalanceError::Fatal(f) => Self::Fatal(f),
        }
    }
}

impl From<VelocityError> for LedgerError {
    fn from(e: VelocityError) -> Self {
        match e {
            VelocityError::Rejected(r) => Self::Rejected(LedgerRejection::Velocity(r)),
            VelocityError::Transient(t) => Self::Transient(t),
            VelocityError::Fatal(f) => Self::Fatal(f),
        }
    }
}

impl From<PostingError> for LedgerError {
    fn from(e: PostingError) -> Self {
        match e {
            PostingError::Rejected(r) => Self::Rejected(LedgerRejection::Posting(r)),
            PostingError::Transient(t) => Self::Transient(t),
            PostingError::Fatal(f) => Self::Fatal(f),
        }
    }
}
