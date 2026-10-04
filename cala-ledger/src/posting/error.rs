use crate::tx_template::error::PrepareTransactionRejection;
use cala_types::{param::*, primitives::*};
use cel_interpreter::*;
use es_entity::errlanes;
use rust_decimal::Decimal;

/// The input responsible for a preparation or direct validation rejection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PostingRef {
    pub index: usize,
    pub tx_id: TransactionId,
}

impl PostingRef {
    pub(super) fn record(self) -> Self {
        let span = tracing::Span::current();
        span.record("failed_posting_index", self.index);
        span.record("failed_posting_id", tracing::field::display(self.tx_id));
        self
    }
}

#[derive(Debug, errlanes::Rejection)]
pub enum AttributedPreparationRejection {
    #[rejection(code = "CALA_POSTING_REJECTED")]
    #[error(
        "Unbalanced transaction: currency {}, layer {:?}, amount {}",
        currency,
        layer,
        amount
    )]
    UnbalancedTransaction {
        posting: PostingRef,
        currency: Currency,
        layer: Layer,
        amount: Decimal,
    },
    #[rejection(code = "CALA_POSTING_REJECTED")]
    #[error("{}", detail)]
    CoreTypeCoercion {
        posting: PostingRef,
        #[source]
        detail: CoreTypeCoercion,
    },
    #[rejection(code = "CALA_POSTING_REJECTED")]
    #[error("Error evaluating expression '{}': {}", expression, source)]
    UnknownIdent {
        posting: PostingRef,
        expression: String,
        #[source]
        source: CelExecutionError,
    },
    #[rejection(code = "CALA_POSTING_REJECTED")]
    #[error("Error evaluating expression '{}': {}", expression, source)]
    MissingArgument {
        posting: PostingRef,
        expression: String,
        #[source]
        source: CelExecutionError,
    },
    #[rejection(code = "CALA_POSTING_REJECTED")]
    #[error("Error evaluating expression '{}': {}", expression, source)]
    NoMatchingOverload {
        posting: PostingRef,
        expression: String,
        #[source]
        source: CelExecutionError,
    },
    #[rejection(code = "CALA_POSTING_REJECTED")]
    #[error("Error evaluating expression '{}': {}", expression, source)]
    Unexpected {
        posting: PostingRef,
        expression: String,
        #[source]
        source: CelExecutionError,
    },
    #[rejection(code = "CALA_POSTING_REJECTED")]
    #[error("Unsupported opaque value {} in '{}'", type_name, expression)]
    UnsupportedOpaque {
        posting: PostingRef,
        expression: String,
        type_name: String,
    },
    #[rejection(code = "CALA_POSTING_REJECTED")]
    #[error("Could not downcast {} in '{}'", type_name, expression)]
    OpaqueDowncast {
        posting: PostingRef,
        expression: String,
        type_name: &'static str,
    },
    #[rejection(code = "CALA_POSTING_REJECTED")]
    #[error("Cannot convert function value in '{}'", expression)]
    FunctionValue {
        posting: PostingRef,
        expression: String,
    },
    #[rejection(code = "CALA_POSTING_REJECTED")]
    #[error("{}", detail)]
    ExternalTypeCoercion {
        posting: PostingRef,
        #[source]
        detail: ExternalTypeCoercion,
    },
    #[rejection(code = "CALA_POSTING_REJECTED")]
    #[error("Invalid currency in '{}': {}", expression, source)]
    InvalidCurrency {
        posting: PostingRef,
        expression: String,
        #[source]
        source: ParseCurrencyError,
    },
    #[rejection(code = "CALA_POSTING_REJECTED")]
    #[error("{}", detail)]
    NonStringKey {
        posting: PostingRef,
        #[source]
        detail: CoreTypeCoercion,
    },
    #[rejection(code = "CALA_POSTING_REJECTED")]
    #[error("Cannot convert bytes to JSON in '{}'", expression)]
    UnsupportedBytes {
        posting: PostingRef,
        expression: String,
    },
    #[rejection(code = "CALA_POSTING_REJECTED")]
    #[error("Error evaluating expression '{}': {}", expression, source)]
    DefaultUnknownIdent {
        posting: PostingRef,
        expression: String,
        #[source]
        source: CelExecutionError,
    },
    #[rejection(code = "CALA_POSTING_REJECTED")]
    #[error("Error evaluating expression '{}': {}", expression, source)]
    DefaultMissingArgument {
        posting: PostingRef,
        expression: String,
        #[source]
        source: CelExecutionError,
    },
    #[rejection(code = "CALA_POSTING_REJECTED")]
    #[error("Error evaluating expression '{}': {}", expression, source)]
    DefaultNoMatchingOverload {
        posting: PostingRef,
        expression: String,
        #[source]
        source: CelExecutionError,
    },
    #[rejection(code = "CALA_POSTING_REJECTED")]
    #[error("Error evaluating expression '{}': {}", expression, source)]
    DefaultUnexpected {
        posting: PostingRef,
        expression: String,
        #[source]
        source: CelExecutionError,
    },
    #[rejection(code = "CALA_POSTING_REJECTED")]
    #[error("Unsupported opaque value {} in '{}'", type_name, expression)]
    DefaultUnsupportedOpaque {
        posting: PostingRef,
        expression: String,
        type_name: String,
    },
    #[rejection(code = "CALA_POSTING_REJECTED")]
    #[error("Could not downcast {} in '{}'", type_name, expression)]
    DefaultOpaqueDowncast {
        posting: PostingRef,
        expression: String,
        type_name: &'static str,
    },
    #[rejection(code = "CALA_POSTING_REJECTED")]
    #[error("Cannot convert function value in '{}'", expression)]
    DefaultFunctionValue {
        posting: PostingRef,
        expression: String,
    },
    #[rejection(code = "CALA_POSTING_REJECTED")]
    #[error("Type mismatch: expected {:?}, got {:?}", expected, actual)]
    TypeMismatch {
        posting: PostingRef,
        expected: ParamDataType,
        actual: CelType,
    },
    #[rejection(code = "CALA_POSTING_REJECTED")]
    #[error("Could not parse {} as Uuid: {}", input, source)]
    InvalidUuid {
        posting: PostingRef,
        input: String,
        #[source]
        source: uuid::Error,
    },
    #[rejection(code = "CALA_POSTING_REJECTED")]
    #[error("Could not parse {} as Decimal: {}", input, source)]
    InvalidDecimal {
        posting: PostingRef,
        input: String,
        #[source]
        source: rust_decimal::Error,
    },
    #[rejection(code = "CALA_POSTING_REJECTED")]
    #[error("Could not parse {} as Date: {}", input, source)]
    InvalidDate {
        posting: PostingRef,
        input: String,
        #[source]
        source: chrono::ParseError,
    },
}

impl AttributedPreparationRejection {
    pub(super) fn from_rejection(
        rejection: PrepareTransactionRejection,
        posting: PostingRef,
    ) -> Self {
        let posting = posting.record();
        match rejection {
            PrepareTransactionRejection::UnbalancedTransaction(currency, layer, amount) => {
                Self::UnbalancedTransaction {
                    posting,
                    currency,
                    layer,
                    amount,
                }
            }
            PrepareTransactionRejection::CoreTypeCoercion(detail) => {
                Self::CoreTypeCoercion { posting, detail }
            }
            PrepareTransactionRejection::UnknownIdent { expression, source } => {
                Self::UnknownIdent {
                    posting,
                    expression,
                    source,
                }
            }
            PrepareTransactionRejection::MissingArgument { expression, source } => {
                Self::MissingArgument {
                    posting,
                    expression,
                    source,
                }
            }
            PrepareTransactionRejection::NoMatchingOverload { expression, source } => {
                Self::NoMatchingOverload {
                    posting,
                    expression,
                    source,
                }
            }
            PrepareTransactionRejection::Unexpected { expression, source } => Self::Unexpected {
                posting,
                expression,
                source,
            },
            PrepareTransactionRejection::UnsupportedOpaque {
                expression,
                type_name,
            } => Self::UnsupportedOpaque {
                posting,
                expression,
                type_name,
            },
            PrepareTransactionRejection::OpaqueDowncast {
                expression,
                type_name,
            } => Self::OpaqueDowncast {
                posting,
                expression,
                type_name,
            },
            PrepareTransactionRejection::FunctionValue { expression } => Self::FunctionValue {
                posting,
                expression,
            },
            PrepareTransactionRejection::ExternalTypeCoercion(detail) => {
                Self::ExternalTypeCoercion { posting, detail }
            }
            PrepareTransactionRejection::InvalidCurrency { expression, source } => {
                Self::InvalidCurrency {
                    posting,
                    expression,
                    source,
                }
            }
            PrepareTransactionRejection::NonStringKey(detail) => {
                Self::NonStringKey { posting, detail }
            }
            PrepareTransactionRejection::UnsupportedBytes { expression } => {
                Self::UnsupportedBytes {
                    posting,
                    expression,
                }
            }
            PrepareTransactionRejection::DefaultUnknownIdent { expression, source } => {
                Self::DefaultUnknownIdent {
                    posting,
                    expression,
                    source,
                }
            }
            PrepareTransactionRejection::DefaultMissingArgument { expression, source } => {
                Self::DefaultMissingArgument {
                    posting,
                    expression,
                    source,
                }
            }
            PrepareTransactionRejection::DefaultNoMatchingOverload { expression, source } => {
                Self::DefaultNoMatchingOverload {
                    posting,
                    expression,
                    source,
                }
            }
            PrepareTransactionRejection::DefaultUnexpected { expression, source } => {
                Self::DefaultUnexpected {
                    posting,
                    expression,
                    source,
                }
            }
            PrepareTransactionRejection::DefaultUnsupportedOpaque {
                expression,
                type_name,
            } => Self::DefaultUnsupportedOpaque {
                posting,
                expression,
                type_name,
            },
            PrepareTransactionRejection::DefaultOpaqueDowncast {
                expression,
                type_name,
            } => Self::DefaultOpaqueDowncast {
                posting,
                expression,
                type_name,
            },
            PrepareTransactionRejection::DefaultFunctionValue { expression } => {
                Self::DefaultFunctionValue {
                    posting,
                    expression,
                }
            }
            PrepareTransactionRejection::TypeMismatch { expected, actual } => Self::TypeMismatch {
                posting,
                expected,
                actual,
            },
            PrepareTransactionRejection::InvalidUuid { input, source } => Self::InvalidUuid {
                posting,
                input,
                source,
            },
            PrepareTransactionRejection::InvalidDecimal { input, source } => Self::InvalidDecimal {
                posting,
                input,
                source,
            },
            PrepareTransactionRejection::InvalidDate { input, source } => Self::InvalidDate {
                posting,
                input,
                source,
            },
        }
    }
}

#[derive(Debug, errlanes::Rejection)]
pub enum PostingValidationRejection {
    #[rejection(code = "CALA_POSTING_ACCOUNT_NOT_FOUND")]
    #[error("AccountNotFound: {}", account_id)]
    AccountNotFound { account_id: AccountId },
    #[rejection(code = "CALA_POSTING_ENTRY_TARGETS_ACCOUNT_SET")]
    #[error("EntryTargetsAccountSet: {}", account_id)]
    EntryTargetsAccountSet { account_id: AccountId },
    #[rejection(code = "CALA_POSTING_ACCOUNT_LOCKED")]
    #[error("AccountLocked: {}", account_id)]
    AccountLocked { account_id: AccountId },
    #[rejection(code = "CALA_POSTING_JOURNAL_NOT_FOUND")]
    #[error("JournalNotFound: {}", journal_id)]
    JournalNotFound { journal_id: JournalId },
    #[rejection(code = "CALA_POSTING_JOURNAL_LOCKED")]
    #[error("JournalLocked: {}", journal_id)]
    JournalLocked { journal_id: JournalId },
}

#[derive(Debug, errlanes::Rejection)]
pub enum ValidateBatchRejection {
    #[rejection(code = "CALA_POSTING_REJECTED")]
    #[error("AccountNotFound: {}", account_id)]
    AccountNotFound {
        posting: PostingRef,
        account_id: AccountId,
    },
    #[rejection(code = "CALA_POSTING_REJECTED")]
    #[error("EntryTargetsAccountSet: {}", account_id)]
    EntryTargetsAccountSet {
        posting: PostingRef,
        account_id: AccountId,
    },
    #[rejection(code = "CALA_POSTING_REJECTED")]
    #[error("AccountLocked: {}", account_id)]
    AccountLocked {
        posting: PostingRef,
        account_id: AccountId,
    },
    #[rejection(code = "CALA_POSTING_REJECTED")]
    #[error("JournalNotFound: {}", journal_id)]
    JournalNotFound {
        posting: PostingRef,
        journal_id: JournalId,
    },
    #[rejection(code = "CALA_POSTING_REJECTED")]
    #[error("JournalLocked: {}", journal_id)]
    JournalLocked {
        posting: PostingRef,
        journal_id: JournalId,
    },
}

impl ValidateBatchRejection {
    pub(super) fn from_rejection(
        rejection: PostingValidationRejection,
        posting: PostingRef,
    ) -> Self {
        let posting = posting.record();
        match rejection {
            PostingValidationRejection::AccountNotFound { account_id } => Self::AccountNotFound {
                posting,
                account_id,
            },
            PostingValidationRejection::EntryTargetsAccountSet { account_id } => {
                Self::EntryTargetsAccountSet {
                    posting,
                    account_id,
                }
            }
            PostingValidationRejection::AccountLocked { account_id } => Self::AccountLocked {
                posting,
                account_id,
            },
            PostingValidationRejection::JournalNotFound { journal_id } => Self::JournalNotFound {
                posting,
                journal_id,
            },
            PostingValidationRejection::JournalLocked { journal_id } => Self::JournalLocked {
                posting,
                journal_id,
            },
        }
    }
}

#[derive(Debug, errlanes::Rejection, errlanes::Lift)]
#[lift(AttributedPreparationRejection)]
pub enum PrepareBatchRejection {
    #[rejection(code = "CALA_POSTING_REJECTED")]
    #[error("Duplicate transaction id within the batch: {}", tx_id)]
    DuplicateTransactionIdInBatch {
        posting: PostingRef,
        tx_id: TransactionId,
    },
    #[rejection(code = "CALA_POSTING_REJECTED")]
    #[error("Duplicate external id within the batch: {}", external_id)]
    DuplicateExternalIdInBatch {
        posting: PostingRef,
        external_id: String,
    },
    #[rejection(code = "CALA_POSTING_REJECTED")]
    #[error(
        "Unbalanced transaction: currency {}, layer {:?}, amount {}",
        currency,
        layer,
        amount
    )]
    #[lift(AttributedPreparationRejection::UnbalancedTransaction)]
    UnbalancedTransaction {
        posting: PostingRef,
        currency: Currency,
        layer: Layer,
        amount: Decimal,
    },
    #[rejection(code = "CALA_POSTING_REJECTED")]
    #[error("{}", detail)]
    #[lift(AttributedPreparationRejection::CoreTypeCoercion)]
    CoreTypeCoercion {
        posting: PostingRef,
        #[source]
        detail: CoreTypeCoercion,
    },
    #[rejection(code = "CALA_POSTING_REJECTED")]
    #[error("Error evaluating expression '{}': {}", expression, source)]
    #[lift(AttributedPreparationRejection::UnknownIdent)]
    UnknownIdent {
        posting: PostingRef,
        expression: String,
        #[source]
        source: CelExecutionError,
    },
    #[rejection(code = "CALA_POSTING_REJECTED")]
    #[error("Error evaluating expression '{}': {}", expression, source)]
    #[lift(AttributedPreparationRejection::MissingArgument)]
    MissingArgument {
        posting: PostingRef,
        expression: String,
        #[source]
        source: CelExecutionError,
    },
    #[rejection(code = "CALA_POSTING_REJECTED")]
    #[error("Error evaluating expression '{}': {}", expression, source)]
    #[lift(AttributedPreparationRejection::NoMatchingOverload)]
    NoMatchingOverload {
        posting: PostingRef,
        expression: String,
        #[source]
        source: CelExecutionError,
    },
    #[rejection(code = "CALA_POSTING_REJECTED")]
    #[error("Error evaluating expression '{}': {}", expression, source)]
    #[lift(AttributedPreparationRejection::Unexpected)]
    Unexpected {
        posting: PostingRef,
        expression: String,
        #[source]
        source: CelExecutionError,
    },
    #[rejection(code = "CALA_POSTING_REJECTED")]
    #[error("Unsupported opaque value {} in '{}'", type_name, expression)]
    #[lift(AttributedPreparationRejection::UnsupportedOpaque)]
    UnsupportedOpaque {
        posting: PostingRef,
        expression: String,
        type_name: String,
    },
    #[rejection(code = "CALA_POSTING_REJECTED")]
    #[error("Could not downcast {} in '{}'", type_name, expression)]
    #[lift(AttributedPreparationRejection::OpaqueDowncast)]
    OpaqueDowncast {
        posting: PostingRef,
        expression: String,
        type_name: &'static str,
    },
    #[rejection(code = "CALA_POSTING_REJECTED")]
    #[error("Cannot convert function value in '{}'", expression)]
    #[lift(AttributedPreparationRejection::FunctionValue)]
    FunctionValue {
        posting: PostingRef,
        expression: String,
    },
    #[rejection(code = "CALA_POSTING_REJECTED")]
    #[error("{}", detail)]
    #[lift(AttributedPreparationRejection::ExternalTypeCoercion)]
    ExternalTypeCoercion {
        posting: PostingRef,
        #[source]
        detail: ExternalTypeCoercion,
    },
    #[rejection(code = "CALA_POSTING_REJECTED")]
    #[error("Invalid currency in '{}': {}", expression, source)]
    #[lift(AttributedPreparationRejection::InvalidCurrency)]
    InvalidCurrency {
        posting: PostingRef,
        expression: String,
        #[source]
        source: ParseCurrencyError,
    },
    #[rejection(code = "CALA_POSTING_REJECTED")]
    #[error("{}", detail)]
    #[lift(AttributedPreparationRejection::NonStringKey)]
    NonStringKey {
        posting: PostingRef,
        #[source]
        detail: CoreTypeCoercion,
    },
    #[rejection(code = "CALA_POSTING_REJECTED")]
    #[error("Cannot convert bytes to JSON in '{}'", expression)]
    #[lift(AttributedPreparationRejection::UnsupportedBytes)]
    UnsupportedBytes {
        posting: PostingRef,
        expression: String,
    },
    #[rejection(code = "CALA_POSTING_REJECTED")]
    #[error("Error evaluating expression '{}': {}", expression, source)]
    #[lift(AttributedPreparationRejection::DefaultUnknownIdent)]
    DefaultUnknownIdent {
        posting: PostingRef,
        expression: String,
        #[source]
        source: CelExecutionError,
    },
    #[rejection(code = "CALA_POSTING_REJECTED")]
    #[error("Error evaluating expression '{}': {}", expression, source)]
    #[lift(AttributedPreparationRejection::DefaultMissingArgument)]
    DefaultMissingArgument {
        posting: PostingRef,
        expression: String,
        #[source]
        source: CelExecutionError,
    },
    #[rejection(code = "CALA_POSTING_REJECTED")]
    #[error("Error evaluating expression '{}': {}", expression, source)]
    #[lift(AttributedPreparationRejection::DefaultNoMatchingOverload)]
    DefaultNoMatchingOverload {
        posting: PostingRef,
        expression: String,
        #[source]
        source: CelExecutionError,
    },
    #[rejection(code = "CALA_POSTING_REJECTED")]
    #[error("Error evaluating expression '{}': {}", expression, source)]
    #[lift(AttributedPreparationRejection::DefaultUnexpected)]
    DefaultUnexpected {
        posting: PostingRef,
        expression: String,
        #[source]
        source: CelExecutionError,
    },
    #[rejection(code = "CALA_POSTING_REJECTED")]
    #[error("Unsupported opaque value {} in '{}'", type_name, expression)]
    #[lift(AttributedPreparationRejection::DefaultUnsupportedOpaque)]
    DefaultUnsupportedOpaque {
        posting: PostingRef,
        expression: String,
        type_name: String,
    },
    #[rejection(code = "CALA_POSTING_REJECTED")]
    #[error("Could not downcast {} in '{}'", type_name, expression)]
    #[lift(AttributedPreparationRejection::DefaultOpaqueDowncast)]
    DefaultOpaqueDowncast {
        posting: PostingRef,
        expression: String,
        type_name: &'static str,
    },
    #[rejection(code = "CALA_POSTING_REJECTED")]
    #[error("Cannot convert function value in '{}'", expression)]
    #[lift(AttributedPreparationRejection::DefaultFunctionValue)]
    DefaultFunctionValue {
        posting: PostingRef,
        expression: String,
    },
    #[rejection(code = "CALA_POSTING_REJECTED")]
    #[error("Type mismatch: expected {:?}, got {:?}", expected, actual)]
    #[lift(AttributedPreparationRejection::TypeMismatch)]
    TypeMismatch {
        posting: PostingRef,
        expected: ParamDataType,
        actual: CelType,
    },
    #[rejection(code = "CALA_POSTING_REJECTED")]
    #[error("Could not parse {} as Uuid: {}", input, source)]
    #[lift(AttributedPreparationRejection::InvalidUuid)]
    InvalidUuid {
        posting: PostingRef,
        input: String,
        #[source]
        source: uuid::Error,
    },
    #[rejection(code = "CALA_POSTING_REJECTED")]
    #[error("Could not parse {} as Decimal: {}", input, source)]
    #[lift(AttributedPreparationRejection::InvalidDecimal)]
    InvalidDecimal {
        posting: PostingRef,
        input: String,
        #[source]
        source: rust_decimal::Error,
    },
    #[rejection(code = "CALA_POSTING_REJECTED")]
    #[error("Could not parse {} as Date: {}", input, source)]
    #[lift(AttributedPreparationRejection::InvalidDate)]
    InvalidDate {
        posting: PostingRef,
        input: String,
        #[source]
        source: chrono::ParseError,
    },
}

#[derive(Debug, errlanes::Rejection)]
pub enum PostWriteRejection {
    #[rejection(code = "CALA_POSTING_DUPLICATETRANSACTIONID")]
    #[error("Transaction id already posted")]
    DuplicateTransactionId,
    #[rejection(code = "CALA_POSTING_DUPLICATEEXTERNALID")]
    #[error("Transaction external id already posted")]
    DuplicateExternalId,
    #[rejection(code = "CALA_POSTING_ENTRYTARGETSACCOUNTSET")]
    #[error("Entry targets an account-set backing account")]
    EntryTargetsAccountSet,
}

#[derive(Debug, errlanes::Rejection)]
#[rejection(code = "BATCH_TOO_MANY_ACCOUNTS")]
#[error(
    "Posting touches {} distinct balances; at most {} may be locked",
    distinct,
    max
)]
pub struct TooManyPostingBalances {
    pub distinct: usize,
    pub max: usize,
}

#[errlanes::compose]
#[derive(Debug)]
pub enum ApplyPostingsRejection {
    #[compose(flatten)]
    Velocity(crate::velocity::error::EnforceVelocityBatchRejection),
    #[compose(flatten)]
    Write(PostWriteRejection),
    #[error("{0}")]
    #[rejection(delegate, from)]
    AncestorAccountLocked(crate::balance::error::BalanceAccountLocked),
}

/// Flat outcomes for one posting.
#[errlanes::compose]
#[derive(Debug)]
pub enum PostingRejection {
    #[compose(flatten)]
    Preparation(AttributedPreparationRejection),
    #[compose(flatten)]
    Validation(ValidateBatchRejection),
    #[compose(flatten)]
    Apply(ApplyPostingsRejection),
    #[error("{0}")]
    #[rejection(delegate, from)]
    TemplateNotFound(crate::tx_template::error::TxTemplateNotFound),
    #[error("{0}")]
    #[rejection(delegate, from)]
    TooManyBalances(TooManyPostingBalances),
}

/// Flat outcomes for a batch of postings.
#[errlanes::compose]
#[derive(Debug)]
pub enum BatchPostingRejection {
    #[compose(flatten)]
    Preparation(PrepareBatchRejection),
    #[compose(flatten)]
    Validation(ValidateBatchRejection),
    #[compose(flatten)]
    Apply(ApplyPostingsRejection),
    #[error("{0}")]
    #[rejection(delegate, from)]
    TemplateNotFound(crate::tx_template::error::TxTemplateNotFound),
    #[error("{0}")]
    #[rejection(delegate, from)]
    TooManyBalances(TooManyPostingBalances),
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
    Domain(PostWriteRejection),
    #[classify(delegate)]
    Sqlx(sqlx::Error),
}
impl From<sqlx::Error> for PostWrite {
    fn from(error: sqlx::Error) -> Self {
        match error.as_database_error().and_then(|e| e.constraint()) {
            Some("cala_transactions_pkey") => {
                Self::Domain(PostWriteRejection::DuplicateTransactionId)
            }
            Some("cala_transactions_external_id_key") => {
                Self::Domain(PostWriteRejection::DuplicateExternalId)
            }
            Some("cala_entries_account_not_account_set_fkey") => {
                Self::Domain(PostWriteRejection::EntryTargetsAccountSet)
            }
            _ => Self::Sqlx(error),
        }
    }
}

#[cfg(test)]
mod contract_tests {
    use super::*;
    use es_entity::errlanes::{Level, Rejection};
    fn single_context(error: PostingRejection) -> Option<PostingRef> {
        match error {
            PostingRejection::PreparationUnbalancedTransaction { posting, .. }
            | PostingRejection::PreparationCoreTypeCoercion { posting, .. }
            | PostingRejection::PreparationUnknownIdent { posting, .. }
            | PostingRejection::PreparationMissingArgument { posting, .. }
            | PostingRejection::PreparationNoMatchingOverload { posting, .. }
            | PostingRejection::PreparationUnexpected { posting, .. }
            | PostingRejection::PreparationUnsupportedOpaque { posting, .. }
            | PostingRejection::PreparationOpaqueDowncast { posting, .. }
            | PostingRejection::PreparationFunctionValue { posting, .. }
            | PostingRejection::PreparationExternalTypeCoercion { posting, .. }
            | PostingRejection::PreparationInvalidCurrency { posting, .. }
            | PostingRejection::PreparationNonStringKey { posting, .. }
            | PostingRejection::PreparationUnsupportedBytes { posting, .. }
            | PostingRejection::PreparationDefaultUnknownIdent { posting, .. }
            | PostingRejection::PreparationDefaultMissingArgument { posting, .. }
            | PostingRejection::PreparationDefaultNoMatchingOverload { posting, .. }
            | PostingRejection::PreparationDefaultUnexpected { posting, .. }
            | PostingRejection::PreparationDefaultUnsupportedOpaque { posting, .. }
            | PostingRejection::PreparationDefaultOpaqueDowncast { posting, .. }
            | PostingRejection::PreparationDefaultFunctionValue { posting, .. }
            | PostingRejection::PreparationTypeMismatch { posting, .. }
            | PostingRejection::PreparationInvalidUuid { posting, .. }
            | PostingRejection::PreparationInvalidDecimal { posting, .. }
            | PostingRejection::PreparationInvalidDate { posting, .. }
            | PostingRejection::ValidationAccountNotFound { posting, .. }
            | PostingRejection::ValidationEntryTargetsAccountSet { posting, .. }
            | PostingRejection::ValidationAccountLocked { posting, .. }
            | PostingRejection::ValidationJournalNotFound { posting, .. }
            | PostingRejection::ValidationJournalLocked { posting, .. } => Some(posting),
            PostingRejection::ApplyVelocityCoreTypeCoercion(..)
            | PostingRejection::ApplyVelocityUnknownIdent { .. }
            | PostingRejection::ApplyVelocityMissingArgument { .. }
            | PostingRejection::ApplyVelocityNoMatchingOverload { .. }
            | PostingRejection::ApplyVelocityUnexpected { .. }
            | PostingRejection::ApplyVelocityUnsupportedOpaque { .. }
            | PostingRejection::ApplyVelocityOpaqueDowncast { .. }
            | PostingRejection::ApplyVelocityFunctionValue { .. }
            | PostingRejection::ApplyVelocityNonStringKey(..)
            | PostingRejection::ApplyVelocityUnsupportedBytes { .. }
            | PostingRejection::ApplyVelocityLimitExceeded(..)
            | PostingRejection::ApplyWriteDuplicateTransactionId
            | PostingRejection::ApplyWriteDuplicateExternalId
            | PostingRejection::ApplyWriteEntryTargetsAccountSet
            | PostingRejection::ApplyAncestorAccountLocked(_)
            | PostingRejection::TemplateNotFound(_)
            | PostingRejection::TooManyBalances(_) => None,
        }
    }
    fn batch_context(error: BatchPostingRejection) -> Option<PostingRef> {
        match error {
            BatchPostingRejection::PreparationDuplicateTransactionIdInBatch { posting, .. }
            | BatchPostingRejection::PreparationDuplicateExternalIdInBatch { posting, .. }
            | BatchPostingRejection::PreparationUnbalancedTransaction { posting, .. }
            | BatchPostingRejection::PreparationCoreTypeCoercion { posting, .. }
            | BatchPostingRejection::PreparationUnknownIdent { posting, .. }
            | BatchPostingRejection::PreparationMissingArgument { posting, .. }
            | BatchPostingRejection::PreparationNoMatchingOverload { posting, .. }
            | BatchPostingRejection::PreparationUnexpected { posting, .. }
            | BatchPostingRejection::PreparationUnsupportedOpaque { posting, .. }
            | BatchPostingRejection::PreparationOpaqueDowncast { posting, .. }
            | BatchPostingRejection::PreparationFunctionValue { posting, .. }
            | BatchPostingRejection::PreparationExternalTypeCoercion { posting, .. }
            | BatchPostingRejection::PreparationInvalidCurrency { posting, .. }
            | BatchPostingRejection::PreparationNonStringKey { posting, .. }
            | BatchPostingRejection::PreparationUnsupportedBytes { posting, .. }
            | BatchPostingRejection::PreparationDefaultUnknownIdent { posting, .. }
            | BatchPostingRejection::PreparationDefaultMissingArgument { posting, .. }
            | BatchPostingRejection::PreparationDefaultNoMatchingOverload { posting, .. }
            | BatchPostingRejection::PreparationDefaultUnexpected { posting, .. }
            | BatchPostingRejection::PreparationDefaultUnsupportedOpaque { posting, .. }
            | BatchPostingRejection::PreparationDefaultOpaqueDowncast { posting, .. }
            | BatchPostingRejection::PreparationDefaultFunctionValue { posting, .. }
            | BatchPostingRejection::PreparationTypeMismatch { posting, .. }
            | BatchPostingRejection::PreparationInvalidUuid { posting, .. }
            | BatchPostingRejection::PreparationInvalidDecimal { posting, .. }
            | BatchPostingRejection::PreparationInvalidDate { posting, .. }
            | BatchPostingRejection::ValidationAccountNotFound { posting, .. }
            | BatchPostingRejection::ValidationEntryTargetsAccountSet { posting, .. }
            | BatchPostingRejection::ValidationAccountLocked { posting, .. }
            | BatchPostingRejection::ValidationJournalNotFound { posting, .. }
            | BatchPostingRejection::ValidationJournalLocked { posting, .. } => Some(posting),
            BatchPostingRejection::ApplyVelocityCoreTypeCoercion(..)
            | BatchPostingRejection::ApplyVelocityUnknownIdent { .. }
            | BatchPostingRejection::ApplyVelocityMissingArgument { .. }
            | BatchPostingRejection::ApplyVelocityNoMatchingOverload { .. }
            | BatchPostingRejection::ApplyVelocityUnexpected { .. }
            | BatchPostingRejection::ApplyVelocityUnsupportedOpaque { .. }
            | BatchPostingRejection::ApplyVelocityOpaqueDowncast { .. }
            | BatchPostingRejection::ApplyVelocityFunctionValue { .. }
            | BatchPostingRejection::ApplyVelocityNonStringKey(..)
            | BatchPostingRejection::ApplyVelocityUnsupportedBytes { .. }
            | BatchPostingRejection::ApplyVelocityLimitExceeded(..)
            | BatchPostingRejection::ApplyWriteDuplicateTransactionId
            | BatchPostingRejection::ApplyWriteDuplicateExternalId
            | BatchPostingRejection::ApplyWriteEntryTargetsAccountSet
            | BatchPostingRejection::ApplyAncestorAccountLocked(_)
            | BatchPostingRejection::TemplateNotFound(_)
            | BatchPostingRejection::TooManyBalances(_) => None,
        }
    }

    #[test]
    fn flat_single_and_batch_contracts_preserve_attribution_and_codes() {
        let posting = PostingRef {
            index: 3,
            tx_id: TransactionId::new(),
        };
        let attributed = || {
            AttributedPreparationRejection::from_rejection(
                PrepareTransactionRejection::UnbalancedTransaction(
                    Currency::USD,
                    Layer::Settled,
                    Decimal::ONE,
                ),
                posting,
            )
        };
        let single = PostingRejection::from(attributed());
        assert_eq!(<&str>::from(single.code()), "CALA_POSTING_REJECTED");
        assert_eq!(single.level(), Level::Info);
        assert!(
            matches!(&single, PostingRejection::PreparationUnbalancedTransaction { currency: Currency::Iso(_), layer: Layer::Settled, amount, .. } if *amount == Decimal::ONE)
        );
        assert_eq!(single_context(single), Some(posting));
        let batch = BatchPostingRejection::from(PrepareBatchRejection::from(attributed()));
        assert_eq!(<&str>::from(batch.code()), "CALA_POSTING_REJECTED");
        assert_eq!(batch_context(batch), Some(posting));
        let duplicate =
            BatchPostingRejection::from(PrepareBatchRejection::DuplicateExternalIdInBatch {
                posting,
                external_id: "duplicate".into(),
            });
        assert_eq!(<&str>::from(duplicate.code()), "CALA_POSTING_REJECTED");
        assert_eq!(batch_context(duplicate), Some(posting));
        let shared = ApplyPostingsRejection::from(PostWriteRejection::DuplicateTransactionId);
        assert_eq!(single_context(shared.into()), None);
        let missing = BatchPostingRejection::from(crate::tx_template::error::TxTemplateNotFound(
            "absent".into(),
        ));
        assert_eq!(batch_context(missing), None);
    }
}

#[cfg(test)]
mod attribution_telemetry_tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    use tracing::{
        field::{Field, Visit},
        span::{Id, Record},
        Subscriber,
    };
    use tracing_subscriber::{layer::Context, prelude::*, Layer as SubscriberLayer};

    #[derive(Clone)]
    struct Capture(Arc<Mutex<Vec<(String, String)>>>);
    struct Visitor<'a>(&'a mut Vec<(String, String)>);
    impl Visit for Visitor<'_> {
        fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
            self.0.push((field.name().into(), format!("{value:?}")));
        }
    }
    impl<S: Subscriber> SubscriberLayer<S> for Capture {
        fn on_record(&self, _: &Id, values: &Record<'_>, _: Context<'_, S>) {
            values.record(&mut Visitor(&mut self.0.lock().unwrap()));
        }
    }

    #[test]
    fn context_adapters_record_each_attribution_field_once() {
        let records = Arc::new(Mutex::new(Vec::new()));
        let subscriber = tracing_subscriber::registry().with(Capture(records.clone()));
        tracing::subscriber::with_default(subscriber, || {
            let span = tracing::info_span!(
                "posting_contract",
                failed_posting_index = tracing::field::Empty,
                failed_posting_id = tracing::field::Empty
            );
            let _guard = span.enter();
            let posting = PostingRef {
                index: 7,
                tx_id: TransactionId::new(),
            };
            let preparation = AttributedPreparationRejection::from_rejection(
                PrepareTransactionRejection::UnbalancedTransaction(
                    Currency::USD,
                    Layer::Settled,
                    Decimal::ONE,
                ),
                posting,
            );
            let _: BatchPostingRejection = PrepareBatchRejection::from(preparation).into();
            let captured = std::mem::take(&mut *records.lock().unwrap());
            assert_eq!(
                captured,
                vec![
                    ("failed_posting_index".into(), "7".into()),
                    ("failed_posting_id".into(), posting.tx_id.to_string())
                ]
            );
            let validation = ValidateBatchRejection::from_rejection(
                PostingValidationRejection::AccountLocked {
                    account_id: AccountId::new(),
                },
                posting,
            );
            let _: PostingRejection = validation.into();
            let captured = records.lock().unwrap();
            assert_eq!(captured.len(), 2);
            assert_eq!(captured[0], ("failed_posting_index".into(), "7".into()));
            assert_eq!(
                captured[1],
                ("failed_posting_id".into(), posting.tx_id.to_string())
            );
        });
    }
}
