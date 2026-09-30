use super::repo::TxTemplateConstraintViolation;
use crate::param::error::ParamRejection;
use crate::primitives::TxTemplateId;
use thiserror::Error;

#[derive(Debug, Error, errlanes::Rejection)]
pub enum TxTemplateLookupRejection {
    #[error("template with code '{0}' not found")]
    NotFoundByCode(String),
}

#[errlanes::compose]
#[derive(Debug, Error)]
#[lift(TxTemplateLookupRejection, strict)]
#[lift(TxTemplateConstraintViolation, unhandled = fatal)]
pub enum TxTemplateRejection {
    #[lift(TxTemplateLookupRejection::NotFoundByCode)]
    #[error("template with code '{0}' not found")]
    NotFoundByCode(String),
    #[error("template code already exists: {0}")]
    #[lift(TxTemplateConstraintViolation::CodeKey)]
    #[rejection(code = "DUPLICATE_CODE")]
    DuplicateCode(#[source] es_entity::ConstraintConflict<String>),
    #[error("template ID already exists: {0}")]
    #[lift(TxTemplateConstraintViolation::Pkey)]
    #[rejection(code = "DUPLICATE_ID")]
    DuplicateId(#[source] es_entity::ConstraintConflict<TxTemplateId>),
}

/// Evaluating a template adds no infrastructure failures or management outcomes.
#[errlanes::compose]
#[derive(Debug, Error)]
#[lift(ParamRejection, strict)]
pub enum TxTemplateEvaluationRejection {
    #[lift(ParamRejection::ParamTypeMismatch)]
    #[error("ParamError - ParamTypeMismatch: {0}")]
    ParamTypeMismatch(String),
    #[lift(ParamRejection::CelError)]
    #[error("ParamError - CelError: {0}")]
    CelError(#[source] cel_interpreter::CelError),
    #[error("unbalanced transaction: currency {0}, layer {1:?}, amount {2}")]
    UnbalancedTransaction(
        crate::primitives::Currency,
        crate::primitives::Layer,
        rust_decimal::Decimal,
    ),
}
impl From<cel_interpreter::CelError> for TxTemplateEvaluationRejection {
    fn from(error: cel_interpreter::CelError) -> Self {
        crate::param::error::ParamRejection::from(error).into()
    }
}

pub type TxTemplateError = errlanes::Fail<TxTemplateRejection, crate::CalaLanes>;
pub type TxTemplateLookupError = errlanes::Fail<TxTemplateLookupRejection, crate::CalaLanes>;
