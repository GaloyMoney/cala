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
    // Called after instrumented preparation returns, so attribution is recorded
    // on the posting span rather than on a nested CEL/template span.
    pub(super) fn record_failure<T>(
        self,
        result: Result<T, PreparePostingRejection>,
    ) -> Result<T, PreparePostingRejection> {
        result.inspect_err(|_| {
            self.record();
        })
    }

    pub(super) fn record(self) -> Self {
        let span = tracing::Span::current();
        span.record("failed_posting_index", self.index);
        span.record("failed_posting_id", tracing::field::display(self.tx_id));
        self
    }
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

/// Rejections for posting one transaction, grouped by the phase that owns them.
#[derive(Debug, errlanes::Rejection)]
pub enum PostingRejection {
    #[error("{0}")]
    #[rejection(delegate, from)]
    Prepare(PreparePostingRejection),
    #[error("{0}")]
    #[rejection(delegate, from)]
    Validate(ValidatePostingRejection),
    #[error("{0}")]
    #[rejection(delegate, from)]
    Apply(ApplyPostingRejection),
}

/// Resolving and evaluating templates, then checking the preparation budget.
#[derive(Debug, errlanes::Rejection)]
pub enum PreparePostingRejection {
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
    /// Evaluating a transaction or entry field, including result conversion.
    #[rejection(code = "CALA_POSTING_REJECTED")]
    #[error("{}", source)]
    Cel {
        posting: PostingRef,
        #[source]
        source: Box<CelConversionRejection>,
    },
    /// Evaluating the default for an omitted parameter.
    #[rejection(code = "CALA_POSTING_REJECTED")]
    #[error("{}", source)]
    Default {
        posting: PostingRef,
        parameter: String,
        #[source]
        source: Box<CelConversionRejection>,
    },
    /// Coercing a supplied parameter to its declared type.
    #[rejection(code = "CALA_POSTING_REJECTED")]
    #[error("{}", source)]
    Param {
        posting: PostingRef,
        parameter: String,
        #[source]
        source: Box<ParamValueRejection>,
    },
    #[error("{0}")]
    #[rejection(delegate, from)]
    TemplateNotFound(crate::tx_template::error::TxTemplateNotFound),
    #[error("{0}")]
    #[rejection(delegate, from)]
    TooManyBalances(TooManyPostingBalances),
}

/// Validating prepared postings against the accounts and journals read under lock.
#[derive(Debug, errlanes::Rejection)]
pub enum ValidatePostingRejection {
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

/// Applying prepared postings, including ancestor locks and velocity enforcement.
#[derive(Debug, errlanes::Rejection)]
pub enum ApplyPostingRejection {
    #[error("{0}")]
    #[rejection(delegate, from)]
    Velocity(crate::velocity::error::EnforceVelocityRejection),
    #[rejection(code = "CALA_POSTING_DUPLICATETRANSACTIONID")]
    #[error("Transaction id already posted")]
    DuplicateTransactionId,
    #[rejection(code = "CALA_POSTING_DUPLICATEEXTERNALID")]
    #[error("Transaction external id already posted")]
    DuplicateExternalId,
    #[rejection(code = "CALA_POSTING_ENTRYTARGETSACCOUNTSET")]
    #[error("Entry targets an account-set backing account")]
    EntryTargetsAccountSet,
    #[error("{0}")]
    #[rejection(delegate, from)]
    AncestorAccountLocked(crate::balance::error::BalanceAccountLocked),
}

/// Batch preparation reuses single-posting preparation and adds cross-input checks.
#[errlanes::compose]
#[derive(Debug)]
pub enum BatchPreparePostingRejection {
    #[compose(flatten)]
    Posting(PreparePostingRejection),
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
}

/// Batch posting has the same phases; only preparation adds batch-specific outcomes.
#[derive(Debug, errlanes::Rejection, errlanes::Lift)]
#[lift(PostingRejection)]
pub enum BatchPostingRejection {
    #[lift(PostingRejection::Prepare, into)]
    #[error("{0}")]
    #[rejection(delegate, from)]
    Prepare(BatchPreparePostingRejection),
    #[lift(PostingRejection::Validate)]
    #[error("{0}")]
    #[rejection(delegate, from)]
    Validate(ValidatePostingRejection),
    #[lift(PostingRejection::Apply)]
    #[error("{0}")]
    #[rejection(delegate, from)]
    Apply(ApplyPostingRejection),
}

// The shared template/locking helpers return the preparation contract. Batch
// posting adds no semantics to those errors beyond including that contract.
impl From<PreparePostingRejection> for BatchPostingRejection {
    fn from(error: PreparePostingRejection) -> Self {
        Self::Prepare(error.into())
    }
}

impl PreparePostingRejection {
    pub(crate) fn unbalanced(
        currency: Currency,
        layer: Layer,
        amount: Decimal,
        posting: PostingRef,
    ) -> Self {
        Self::UnbalancedTransaction {
            posting,
            currency,
            layer,
            amount,
        }
    }
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
    Domain(ApplyPostingRejection),
    #[classify(delegate)]
    Sqlx(sqlx::Error),
}
impl From<sqlx::Error> for PostWrite {
    fn from(error: sqlx::Error) -> Self {
        match error.as_database_error().and_then(|e| e.constraint()) {
            Some("cala_transactions_pkey") => {
                Self::Domain(ApplyPostingRejection::DuplicateTransactionId)
            }
            Some("cala_transactions_external_id_key") => {
                Self::Domain(ApplyPostingRejection::DuplicateExternalId)
            }
            Some("cala_entries_account_not_account_set_fkey") => {
                Self::Domain(ApplyPostingRejection::EntryTargetsAccountSet)
            }
            _ => Self::Sqlx(error),
        }
    }
}

#[cfg(test)]
mod contract_tests {
    use super::*;
    use es_entity::errlanes::{lanes, Fail, Level, Rejection, ResultExt};
    use std::error::Error;

    fn preparation_context(error: PreparePostingRejection) -> Option<PostingRef> {
        match error {
            PreparePostingRejection::UnbalancedTransaction { posting, .. }
            | PreparePostingRejection::Cel { posting, .. }
            | PreparePostingRejection::Default { posting, .. }
            | PreparePostingRejection::Param { posting, .. } => Some(posting),
            PreparePostingRejection::TemplateNotFound(_)
            | PreparePostingRejection::TooManyBalances(_) => None,
        }
    }

    fn validation_context(error: ValidatePostingRejection) -> PostingRef {
        match error {
            ValidatePostingRejection::AccountNotFound { posting, .. }
            | ValidatePostingRejection::EntryTargetsAccountSet { posting, .. }
            | ValidatePostingRejection::AccountLocked { posting, .. }
            | ValidatePostingRejection::JournalNotFound { posting, .. }
            | ValidatePostingRejection::JournalLocked { posting, .. } => posting,
        }
    }

    fn single_context(error: PostingRejection) -> Option<PostingRef> {
        match error {
            PostingRejection::Prepare(error) => preparation_context(error),
            PostingRejection::Validate(error) => Some(validation_context(error)),
            PostingRejection::Apply(_) => None,
        }
    }

    fn batch_context(error: BatchPostingRejection) -> Option<PostingRef> {
        match error {
            BatchPostingRejection::Prepare(error) => match error {
                BatchPreparePostingRejection::DuplicateTransactionIdInBatch { posting, .. }
                | BatchPreparePostingRejection::DuplicateExternalIdInBatch { posting, .. }
                | BatchPreparePostingRejection::PostingUnbalancedTransaction { posting, .. }
                | BatchPreparePostingRejection::PostingCel { posting, .. }
                | BatchPreparePostingRejection::PostingDefault { posting, .. }
                | BatchPreparePostingRejection::PostingParam { posting, .. } => Some(posting),
                BatchPreparePostingRejection::PostingTemplateNotFound(_)
                | BatchPreparePostingRejection::PostingTooManyBalances(_) => None,
            },
            BatchPostingRejection::Validate(error) => Some(validation_context(error)),
            BatchPostingRejection::Apply(_) => None,
        }
    }

    #[test]
    fn single_and_batch_phases_preserve_attribution_and_codes() {
        let posting = PostingRef {
            index: 3,
            tx_id: TransactionId::new(),
        };
        let attributed = || {
            PostingRejection::from(PreparePostingRejection::unbalanced(
                Currency::USD,
                Layer::Settled,
                Decimal::ONE,
                posting,
            ))
        };
        let single = attributed();
        assert_eq!(<&str>::from(single.code()), "CALA_POSTING_REJECTED");
        assert_eq!(single.level(), Level::Info);
        assert!(matches!(&single, PostingRejection::Prepare(
            PreparePostingRejection::UnbalancedTransaction { currency: Currency::Iso(_), layer: Layer::Settled, amount, .. }
        ) if *amount == Decimal::ONE));
        assert_eq!(single_context(single), Some(posting));
        let batch = BatchPostingRejection::from(attributed());
        assert_eq!(<&str>::from(batch.code()), "CALA_POSTING_REJECTED");
        assert_eq!(batch_context(batch), Some(posting));

        let validation = || {
            PostingRejection::from(ValidatePostingRejection::AccountLocked {
                posting,
                account_id: AccountId::new(),
            })
        };
        assert_eq!(single_context(validation()), Some(posting));
        assert_eq!(batch_context(validation().into()), Some(posting));

        let duplicate = BatchPreparePostingRejection::DuplicateExternalIdInBatch {
            posting,
            external_id: "duplicate".into(),
        };
        let batch = BatchPostingRejection::from(duplicate);
        assert_eq!(<&str>::from(batch.code()), "CALA_POSTING_REJECTED");
        assert_eq!(batch_context(batch), Some(posting));
        let shared = PostingRejection::from(ApplyPostingRejection::DuplicateTransactionId);
        assert_eq!(single_context(shared), None);
        let missing = BatchPostingRejection::from(PreparePostingRejection::from(
            crate::tx_template::error::TxTemplateNotFound("absent".into()),
        ));
        assert!(matches!(
            &missing,
            BatchPostingRejection::Prepare(BatchPreparePostingRejection::PostingTemplateNotFound(
                _
            ))
        ));
        assert_eq!(batch_context(missing), None);
    }

    #[test]
    fn external_parse_failure_keeps_source_and_attribution_through_posting() {
        let posting = PostingRef {
            index: 2,
            tx_id: TransactionId::new(),
        };
        let attributed = || {
            let expression: CelExpression = "'INVALID'".parse().unwrap();
            let error = expression
                .try_evaluate::<Currency>(&CelContext::new())
                .unwrap_err();
            PostingRejection::from(PreparePostingRejection::Cel {
                posting,
                source: Box::new(error),
            })
        };
        let single = attributed();
        let batch = BatchPostingRejection::from(attributed());
        assert!(single.source().unwrap().is::<PreparePostingRejection>());
        assert!(batch.source().unwrap().is::<BatchPreparePostingRejection>());
        for error in [&single as &dyn Error, &batch as &dyn Error] {
            let source = error.source().unwrap().source().unwrap();
            assert!(source.is::<Box<CelConversionRejection>>());
            let detail = source
                .source()
                .unwrap()
                .downcast_ref::<ExternalParseError>()
                .unwrap();
            assert_eq!(detail.type_name, "currency");
            assert!(matches!(
                detail.source().unwrap().downcast_ref::<ParseCurrencyError>(),
                Some(ParseCurrencyError::UnknownCurrency(value)) if value == "INVALID"
            ));
        }
        assert_eq!(<&str>::from(single.code()), "CALA_POSTING_REJECTED");
        assert_eq!(<&str>::from(batch.code()), "CALA_POSTING_REJECTED");
        assert_eq!(single_context(single), Some(posting));
        assert_eq!(batch_context(batch), Some(posting));
    }

    #[test]
    fn velocity_cel_failure_widens_through_apply_without_changing_diagnostics() {
        use crate::velocity::error::EnforceVelocityRejection;
        let expression: CelExpression = "missing_variable".parse().unwrap();
        let source = expression.evaluate(&CelContext::new()).unwrap_err();
        let result: Result<(), EnforceVelocityRejection> = Err(source.into());
        let apply = result.widen::<Fail<ApplyPostingRejection, lanes!(Transient, Fatal)>>();
        let posting = apply.widen::<Fail<PostingRejection, lanes!(Transient, Fatal)>>();
        let batch = posting
            .widen::<Fail<BatchPostingRejection, lanes!(Transient, Fatal)>>()
            .unwrap_err()
            .rejected()
            .unwrap();
        assert_eq!(<&str>::from(batch.code()), "CEL_EVALUATION_ERROR");
        assert_eq!(batch.level(), Level::Info);
        assert!(matches!(&batch, BatchPostingRejection::Apply(
            ApplyPostingRejection::Velocity(EnforceVelocityRejection::Cel(
                CelConversionRejection::UnknownIdent { expression, .. }
            ))
        ) if expression == "missing_variable"));
        let velocity = batch.source().unwrap().source().unwrap();
        assert!(velocity.is::<EnforceVelocityRejection>());
        let cel = velocity.source().unwrap();
        assert!(cel.is::<CelConversionRejection>());
        assert!(cel.source().unwrap().is::<CelExecutionError>());
        assert_eq!(batch_context(batch), None);
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
    fn preparation_records_attribution_after_leaving_nested_spans() {
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
            let preparation: Result<(), PreparePostingRejection> = {
                let child = tracing::info_span!("template_preparation");
                let _child = child.enter();
                Err(PreparePostingRejection::unbalanced(
                    Currency::USD,
                    Layer::Settled,
                    Decimal::ONE,
                    posting,
                ))
            };
            assert!(records.lock().unwrap().is_empty());
            let error = posting.record_failure(preparation).unwrap_err();
            let _: BatchPostingRejection = error.into();
            let captured = std::mem::take(&mut *records.lock().unwrap());
            assert_eq!(
                captured,
                vec![
                    ("failed_posting_index".into(), "7".into()),
                    ("failed_posting_id".into(), posting.tx_id.to_string())
                ]
            );
            let validation = ValidatePostingRejection::AccountLocked {
                account_id: AccountId::new(),
                posting: posting.record(),
            };
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
