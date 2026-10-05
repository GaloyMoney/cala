use thiserror::Error;

/// Posting failures are the same contract at the ledger and posting boundaries.
pub use crate::posting::{
    PostingError as LedgerError, PostingRejection as LedgerRejection,
    PostingRejectionSchema as LedgerRejectionSchema,
};

pub(super) fn config_error(message: impl Into<String>) -> crate::CalaFault {
    errlanes::Fatal::new(errlanes::FatalKind::Config)
        .with_context(message.into())
        .into()
}
pub(super) fn migration_error(error: sqlx::migrate::MigrateError) -> crate::CalaFault {
    errlanes::Fatal::from_error(errlanes::FatalKind::Config, error).into()
}

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

pub(crate) fn subscription_error(error: obix::out::SubscriptionError) -> crate::CalaFault {
    match error {
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
            errlanes::Transient::new(errlanes::TransientKind::Congestion)
                .with_context(detail.to_string())
                .with_source(detail)
                .into()
        }
        other => errlanes::Fatal::from_error(errlanes::FatalKind::Dependency, other).into(),
    }
}
