use thiserror::Error;

use cel_interpreter::CelError;

/// Params carries no infrastructure failures of its own — evaluating a CEL
/// expression against caller-supplied params is a pure computation — so
/// there is no `ParamError` wrapper: every failure here is a domain
/// rejection a caller can act on (fix the input and retry), and modules
/// that embed params (`tx_template`, `velocity`) lift it directly into
/// their own rejection enum.
// Not `Clone`: `CelError` (from `cala-cel-interpreter`, out of this
// rollout's scope) does not implement it.
#[derive(Debug, Error, errlanes::Rejection)]
pub enum ParamRejection {
    #[error("ParamError - ParamTypeMismatch: {0}")]
    ParamTypeMismatch(String),
    #[error("ParamError - CelError: {0}")]
    CelError(#[from] CelError),
}
