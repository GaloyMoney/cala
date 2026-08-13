use thiserror::Error;

use crate::primitives::{AccountId, TransactionId};

/// A failure attributed to a specific posting within a batch.
///
/// The batch API is all-or-nothing: the whole operation aborts on the first
/// failure. `index` and `tx_id` identify which posting of the submitted batch
/// caused it, so a caller can eject the offender and retry the remainder
/// without having to correlate an opaque error against its input.
#[derive(Error, Debug)]
#[error("posting {index} ({tx_id}) failed: {source}")]
pub struct PostingError {
    pub index: usize,
    pub tx_id: TransactionId,
    #[source]
    pub source: Box<PostingErrorKind>,
}

impl PostingError {
    pub(super) fn at(
        index: usize,
        tx_id: TransactionId,
        source: impl Into<PostingErrorKind>,
    ) -> Self {
        Self {
            index,
            tx_id,
            source: Box::new(source.into()),
        }
    }
}

/// The business-level reason a posting was rejected.
///
/// Every variant is detected **client-side, before the apply statement runs**,
/// which is what keeps the failure attributable to one posting: nothing has
/// been written when it surfaces. Infrastructure failures (constraint races,
/// deadlocks, connection loss) are not attributable and surface as
/// [`crate::ledger::error::LedgerError`] directly.
#[derive(Error, Debug)]
pub enum PostingErrorKind {
    #[error("{0}")]
    TxTemplate(#[from] crate::tx_template::error::TxTemplateError),
    #[error("{0}")]
    Velocity(#[from] crate::velocity::error::VelocityError),
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
    JournalLocked(crate::primitives::JournalId),
    #[error("journal {0} does not exist")]
    JournalNotFound(crate::primitives::JournalId),
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
