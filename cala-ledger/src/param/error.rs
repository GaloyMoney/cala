use thiserror::Error;

use cel_interpreter::CelError;

#[derive(Error, Debug)]
pub enum ParamError {
    #[error("ParamError - ParamTypeMismatch: {0}")]
    ParamTypeMismatch(String),
    #[error("ParamError - TooManyParameters")]
    TooManyParameters,
    #[error("ParamError - CelError: {0}")]
    CelError(#[from] CelError),
}
