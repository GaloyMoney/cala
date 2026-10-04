use es_entity::errlanes;

use cel_interpreter::CelError;

#[derive(errlanes::Rejection, Debug)]
pub enum ParamRejection {
    #[error("ParamRejection - ParamTypeMismatch: {0}")]
    ParamTypeMismatch(String),
    #[error("ParamRejection - CelError: {0}")]
    #[rejection(from)]
    CelError(CelError),
}

impl From<String> for ParamRejection {
    fn from(message: String) -> Self {
        Self::ParamTypeMismatch(message)
    }
}
