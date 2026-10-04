use es_entity::errlanes;

use crate::{
    balance::error::BalanceRejection,
    primitives::{AccountId, JournalId, TransactionId},
    tx_template::error::TxTemplateRejection,
    velocity::error::VelocityRejection,
};

/// Caller-correctable posting outcomes. Infrastructure failures travel in
/// the fault lanes. `Rejected` identifies an offending input within a batch.
#[derive(errlanes::Rejection, Debug)]
pub enum PostingRejection {
    #[error("transaction id already posted")]
    #[rejection(code = "CALA_POSTING_DUPLICATETRANSACTIONID")]
    DuplicateTransactionId,
    #[error("transaction external id already posted")]
    #[rejection(code = "CALA_POSTING_DUPLICATEEXTERNALID")]
    DuplicateExternalId,
    #[error("entry targets an account-set backing account")]
    #[rejection(code = "CALA_POSTING_ENTRYTARGETSACCOUNTSET")]
    EntryTargetsAccountSet,

    #[error("PostingRejection - TxTemplateRejection: {0}")]
    #[rejection(delegate, from)]
    TxTemplateRejection(TxTemplateRejection),
    #[error("PostingRejection - VelocityRejection: {0}")]
    #[rejection(delegate, from)]
    VelocityRejection(VelocityRejection),
    #[error("PostingRejection - BalanceRejection: {0}")]
    #[rejection(delegate, from)]
    BalanceRejection(BalanceRejection),
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
    /// and surface through fault lanes. Known uniqueness conflicts remain
    /// unattributed rejections.
    #[error("PostingRejection - Rejected: posting {index} ({tx_id}): {reason}")]
    #[rejection(code = "CALA_POSTING_REJECTED")]
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
        "PostingRejection - BatchTooManyAccounts: this batch touches {distinct} distinct \
         (journal, account, currency) balances; at most {max} may be locked in one batch. \
         Split it — batch *size* is not the limit, the number of distinct accounts is."
    )]
    BatchTooManyAccounts { distinct: usize, max: usize },
}

impl PostingRejection {
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
        Self::Rejected {
            index,
            tx_id,
            reason: Box::new(reason.into()),
        }
    }
}

/// The business-level reason a posting was rejected.
#[derive(errlanes::Rejection, Debug)]
pub enum RejectionReason {
    #[error("{0}")]
    #[rejection(delegate, from)]
    TxTemplate(TxTemplateRejection),
    #[error("account {0} does not exist")]
    #[rejection(code = "CALA_POSTING_ACCOUNT_NOT_FOUND")]
    AccountNotFound(AccountId),
    #[error(
        "an entry may not be posted directly to an account-set backing account \
         ({0}); an account set's balance is derived from its members"
    )]
    #[rejection(code = "CALA_POSTING_ENTRY_TARGETS_ACCOUNT_SET")]
    EntryTargetsAccountSet(AccountId),
    #[error("account {0} is locked")]
    #[rejection(code = "CALA_POSTING_ACCOUNT_LOCKED")]
    AccountLocked(AccountId),
    #[error("journal {0} is locked")]
    #[rejection(code = "CALA_POSTING_JOURNAL_LOCKED")]
    JournalLocked(JournalId),
    #[error("journal {0} does not exist")]
    #[rejection(code = "CALA_POSTING_JOURNAL_NOT_FOUND")]
    JournalNotFound(JournalId),
    #[error("duplicate transaction id {0} within the submitted batch")]
    #[rejection(code = "CALA_POSTING_DUPLICATE_TRANSACTION_ID_IN_BATCH")]
    DuplicateTransactionIdInBatch(TransactionId),
    #[error("duplicate external id `{0}` within the submitted batch")]
    #[rejection(code = "CALA_POSTING_DUPLICATE_EXTERNAL_ID_IN_BATCH")]
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

/// Classifies this write's known constraints; every other SQL failure keeps its native lane.
#[derive(Debug, errlanes::Classify)]
pub(crate) enum PostWrite {
    #[classify(delegate)]
    Domain(PostingRejection),
    #[classify(delegate)]
    Sqlx(sqlx::Error),
}
impl From<sqlx::Error> for PostWrite {
    fn from(error: sqlx::Error) -> Self {
        match error.as_database_error().and_then(|e| e.constraint()) {
            Some("cala_transactions_pkey") => {
                Self::Domain(PostingRejection::DuplicateTransactionId)
            }
            Some("cala_transactions_external_id_key") => {
                Self::Domain(PostingRejection::DuplicateExternalId)
            }
            Some("cala_entries_account_not_account_set_fkey") => {
                Self::Domain(PostingRejection::EntryTargetsAccountSet)
            }
            _ => Self::Sqlx(error),
        }
    }
}
