use crate::primitives::{AccountId, JournalId, TransactionId};
use thiserror::Error;

/// Rejections from posting. Batch attribution is retained; module-only paths are flattened.
#[errlanes::compose]
#[derive(Debug, Error)]
pub enum PostingRejection {
    #[error("cannot update balances: the account {0} is locked")]
    AccountLocked(AccountId),
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
    #[compose(flatten)]
    TxTemplate(crate::tx_template::error::TxTemplateLookupRejection),
    #[compose(flatten)]
    Velocity(crate::velocity::error::VelocityEnforcementRejection),
}

pub type PostingError = errlanes::Fail<PostingRejection, crate::CalaLanes>;

pub(super) fn rejected(
    index: usize,
    tx_id: TransactionId,
    reason: impl Into<RejectionReason>,
) -> PostingError {
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

/// The transaction insert owns the duplicate-ID interpretation of this constraint.
pub(super) fn transaction_write_error(error: sqlx::Error) -> PostingError {
    if let sqlx::Error::Database(db) = &error {
        if db.is_unique_violation() && db.constraint() == Some("cala_transactions_pkey") {
            return PostingRejection::DuplicateTransactionId.into();
        }
    }
    error.into()
}

#[errlanes::compose]
#[derive(Debug, Error)]
pub enum RejectionReason {
    #[compose(flatten)]
    TxTemplate(crate::tx_template::error::TxTemplateEvaluationRejection),
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
