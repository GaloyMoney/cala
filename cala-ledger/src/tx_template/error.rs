use rust_decimal::Decimal;
use thiserror::Error;

use cala_types::primitives::{Currency, Layer};
use cel_interpreter::CelError;

use crate::error_support::{impl_lane_error_fail, impl_lane_error_fault};
use crate::param::error::ParamRejection;

use super::repo::{TxTemplateConstraint, TxTemplateConstraintViolation};

// Not `Clone`: carries `CelError` (from `cala-cel-interpreter`, out of this
// rollout's scope), which does not implement it.
#[derive(Debug, Error, errlanes::Rejection)]
#[rejection(lift(TxTemplateConstraintViolation))]
pub enum TxTemplateRejection {
    /// Also raised when the posting flow references a template by a code
    /// that does not exist (`posting::template_cache`) — the collapsed
    /// form of what used to be a separate unit `NotFound` variant on top
    /// of this one; the two were never distinguishable to a caller.
    #[error("template with code '{0}' not found")]
    NotFoundByCode(String),
    #[error("code '{0}' already exists")]
    #[rejection(key = TxTemplateConstraint::CodeKey, with = code_taken)]
    DuplicateCode(String),
    #[error("id '{0}' already exists")]
    #[rejection(key = TxTemplateConstraint::Pkey, with = id_taken)]
    DuplicateId(String),
    /// Not `#[from]`: see `param::error::ParamRejection::CelError` — the
    /// wrapped type would need to implement `errlanes::Rejection`, and
    /// `CelError` is out of this rollout's scope.
    #[error("TxTemplateError - CelError: {0}")]
    CelError(CelError),
    #[error(transparent)]
    Param(#[from] ParamRejection),
    #[error("TxTemplateError - UnbalancedTransaction: currency {0}, layer {1:?}, amount {2}")]
    UnbalancedTransaction(Currency, Layer, Decimal),
}

fn code_taken(cv: TxTemplateConstraintViolation) -> TxTemplateRejection {
    TxTemplateRejection::DuplicateCode(cv.value().unwrap_or_default().to_owned())
}

fn id_taken(cv: TxTemplateConstraintViolation) -> TxTemplateRejection {
    TxTemplateRejection::DuplicateId(cv.value().unwrap_or_default().to_owned())
}

#[derive(Debug, Error)]
pub enum TxTemplateError {
    #[error(transparent)]
    Rejected(#[from] TxTemplateRejection),
    #[error(transparent)]
    Transient(#[from] errlanes::Transient),
    #[error(transparent)]
    Fatal(#[from] errlanes::Fatal),
}

impl_lane_error_fault!(TxTemplateError);
impl_lane_error_fail!(
    TxTemplateError,
    TxTemplateRejection,
    TxTemplateConstraintViolation
);

/// `CelExpression::try_evaluate` (template-body evaluation) is called
/// directly at many sites in `tx_template::mod`, every one propagating
/// with `?`. A direct hop here keeps all of them unchanged rather than
/// wrapping each in `.map_err`.
impl From<CelError> for TxTemplateError {
    fn from(e: CelError) -> Self {
        Self::Rejected(TxTemplateRejection::CelError(e))
    }
}

/// `Params::into_context` returns a bare `ParamRejection` (params carry no
/// infrastructure failures of their own); this direct hop is what lets its
/// call site keep using `?` without an intermediate `.map_err`.
impl From<ParamRejection> for TxTemplateError {
    fn from(e: ParamRejection) -> Self {
        Self::Rejected(TxTemplateRejection::Param(e))
    }
}

/// A stored template body that no longer deserializes is corrupt state,
/// not a caller-correctable outcome — the template was valid when it was
/// written; something rewrote or truncated the stored event.
impl From<serde_json::Error> for TxTemplateError {
    fn from(e: serde_json::Error) -> Self {
        Self::Fatal(errlanes::Fatal::from_error(
            errlanes::FatalKind::CorruptState,
            e,
        ))
    }
}
