use thiserror::Error;

use crate::{
    account_set::error::{AccountSetError, AccountSetRejection},
    balance::error::{BalanceError, BalanceRejection},
    error_support::impl_lane_error_fault_only,
    primitives::{AccountId, JournalId, TransactionId},
    tx_template::error::{TxTemplateError, TxTemplateRejection},
    velocity::error::{VelocityError, VelocityRejection},
};

/// The posting module's rejection, nested under
/// [`crate::ledger::error::LedgerRejection`], never the other way around,
/// exactly like every other domain rejection.
///
/// Domain rejections the flow passes through keep their own granularity
/// via `#[from]`; failures specific to the posting path get their own
/// variants here. [`Self::Rejected`] additionally attributes a rejection to
/// one posting of the submitted batch.
#[derive(Debug, Error, errlanes::Rejection)]
pub enum PostingRejection {
    /// The `cala_transactions` primary key rejected a write already
    /// present in the table — an idempotent replay is the caller's to
    /// interpret, so it is not attributed to a specific posting in the
    /// batch the way [`Self::Rejected`] is.
    #[error("duplicate transaction id in this batch")]
    DuplicateTransactionId,
    /// A failure attributed to a specific posting within a batch.
    ///
    /// The batch API is all-or-nothing: the whole operation aborts on the
    /// first failure. `index` and `tx_id` identify which posting of the
    /// submitted batch caused it, so a caller can eject the offender and
    /// retry the remainder without correlating an opaque error against its
    /// input. Every reason is detected **client-side, before the apply
    /// statement runs**, which is what keeps the failure attributable:
    /// nothing has been written when it surfaces. Infrastructure failures
    /// (constraint races, deadlocks, connection loss) are not attributable
    /// and surface through the other variants.
    #[error("posting {index} ({tx_id}): {reason}")]
    Rejected {
        index: usize,
        tx_id: TransactionId,
        reason: Box<RejectionReason>,
    },
    /// The batch would hold more advisory locks than the shared lock table can
    /// be relied on to provide. Refused up front, because the alternative is a
    /// bare `out of shared memory` from Postgres that names neither the cause
    /// nor the fix — and that can strike unrelated concurrent transactions too.
    #[error(
        "this batch touches {distinct} distinct (journal, account, currency) balances; \
         at most {max} may be locked in one batch. Split it — batch *size* is not the \
         limit, the number of distinct accounts is."
    )]
    BatchTooManyAccounts { distinct: usize, max: usize },
    #[error(transparent)]
    TxTemplate(#[from] TxTemplateRejection),
    #[error(transparent)]
    Velocity(#[from] VelocityRejection),
    #[error(transparent)]
    AccountSet(#[from] AccountSetRejection),
    #[error(transparent)]
    Balance(#[from] BalanceRejection),
}

#[derive(Debug, Error)]
pub enum PostingError {
    #[error(transparent)]
    Rejected(#[from] PostingRejection),
    #[error(transparent)]
    Transient(#[from] errlanes::Transient),
    #[error(transparent)]
    Fatal(#[from] errlanes::Fatal),
}

impl PostingError {
    pub(super) fn rejected(
        index: usize,
        tx_id: TransactionId,
        reason: impl Into<RejectionReason>,
    ) -> Self {
        // Keep the attribution observable on the flow's span even when the
        // caller only logs the error.
        let span = tracing::Span::current();
        span.record("failed_posting_index", index);
        span.record("failed_posting_id", tracing::field::display(tx_id));
        PostingRejection::Rejected {
            index,
            tx_id,
            reason: Box::new(reason.into()),
        }
        .into()
    }
}

impl_lane_error_fault_only!(PostingError);

/// `cala_transactions`'s primary key is the only constraint this module's
/// hand-written SQL treats as a rejection rather than `Fatal(Invariant)` —
/// everything else (a `23`-class violation on some other constraint, a
/// deadlock, connection loss, ...) goes through the generic classifier.
const TRANSACTIONS_PKEY_CONSTRAINT: &str = "cala_transactions_pkey";

impl From<sqlx::Error> for PostingError {
    fn from(e: sqlx::Error) -> Self {
        if let sqlx::Error::Database(ref db_err) = e {
            if db_err.constraint() == Some(TRANSACTIONS_PKEY_CONSTRAINT) {
                return PostingRejection::DuplicateTransactionId.into();
            }
        }
        errlanes::Fault::from(e).into()
    }
}

impl From<TxTemplateError> for PostingError {
    fn from(e: TxTemplateError) -> Self {
        match e {
            TxTemplateError::Rejected(r) => Self::Rejected(PostingRejection::TxTemplate(r)),
            TxTemplateError::Transient(t) => Self::Transient(t),
            TxTemplateError::Fatal(f) => Self::Fatal(f),
        }
    }
}

impl From<VelocityError> for PostingError {
    fn from(e: VelocityError) -> Self {
        match e {
            VelocityError::Rejected(r) => Self::Rejected(PostingRejection::Velocity(r)),
            VelocityError::Transient(t) => Self::Transient(t),
            VelocityError::Fatal(f) => Self::Fatal(f),
        }
    }
}

impl From<AccountSetError> for PostingError {
    fn from(e: AccountSetError) -> Self {
        match e {
            AccountSetError::Rejected(r) => Self::Rejected(PostingRejection::AccountSet(r)),
            AccountSetError::Transient(t) => Self::Transient(t),
            AccountSetError::Fatal(f) => Self::Fatal(f),
        }
    }
}

impl From<BalanceError> for PostingError {
    fn from(e: BalanceError) -> Self {
        match e {
            BalanceError::Rejected(r) => Self::Rejected(PostingRejection::Balance(r)),
            BalanceError::Transient(t) => Self::Transient(t),
            BalanceError::Fatal(f) => Self::Fatal(f),
        }
    }
}

/// The business-level reason a posting was rejected.
#[derive(Debug, Error)]
pub enum RejectionReason {
    #[error(transparent)]
    TxTemplate(#[from] TxTemplateRejection),
    #[error("account {0} does not exist")]
    AccountNotFound(AccountId),
    #[error(
        "an entry may not be posted directly to an account-set backing account \
         ({0}); an account set's balance is derived from its members"
    )]
    EntryTargetsAccountSet(AccountId),
    #[error("account {0} is locked")]
    AccountLocked(AccountId),
    #[error("journal {0} is locked")]
    JournalLocked(JournalId),
    #[error("journal {0} does not exist")]
    JournalNotFound(JournalId),
    #[error("duplicate transaction id {0} within the submitted batch")]
    DuplicateTransactionIdInBatch(TransactionId),
    #[error("duplicate external id `{0}` within the submitted batch")]
    DuplicateExternalIdInBatch(String),
}

/// The number of distinct `(journal, account, currency)` triples one batch may
/// lock.
///
/// The fence takes two advisory locks per distinct entry account — a shared
/// class-1 lock and, for non-EC accounts, a per-balance exclusive — and holds
/// them all until commit. Advisory locks live in the *shared* lock table, sized
/// `max_locks_per_transaction x (max_connections + max_prepared_transactions)`,
/// so a batch spanning enough distinct accounts exhausts it and Postgres aborts
/// with a bare `out of shared memory`, which says nothing about the cause and
/// can equally be triggered by unrelated concurrent work.
///
/// Batch *size* is not the constraint — 500k postings over a small account pool
/// lock only that pool. Distinct accounts are. This bound is deliberately well
/// under the stock ceiling (64 x 100 = 6400 slots) because the table is shared
/// with every other backend; a batch that fits alone can still fail beside
/// concurrent traffic.
pub(super) const MAX_DISTINCT_BALANCES_PER_BATCH: usize = 1_000;
