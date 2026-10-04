use super::repo::TxTemplateConstraintViolation;
use es_entity::errlanes;
use rust_decimal::Decimal;

use cala_types::primitives::{Currency, Layer};
use cel_interpreter::CelError;

#[derive(errlanes::Rejection, errlanes::Lift, Debug)]
#[lift(TxTemplateConstraintViolation, unhandled = fatal)]
pub enum TxTemplateRejection {
    #[error("TxTemplateRejection - DuplicateCode: code '{0:?}' already exists")]
    #[rejection(code = "CALA_TX_TEMPLATE_DUPLICATE_CODE")]
    #[lift(TxTemplateConstraintViolation::CodeKey, field = attempted)]
    DuplicateCode(Option<String>),
    #[error("TxTemplateRejection - DuplicateId: id '{0:?}' already exists")]
    #[rejection(code = "CALA_TX_TEMPLATE_DUPLICATE_ID")]
    #[lift(TxTemplateConstraintViolation::Pkey, field = attempted)]
    DuplicateId(crate::TxTemplateId),
    #[error("TxTemplateRejection - CelError: {0}")]
    #[rejection(delegate, from)]
    CelError(CelError),
    #[error("TxTemplateRejection - UnbalancedTransaction: currency {0}, layer {1:?}, amount {2}")]
    #[rejection(code = "CALA_TX_TEMPLATE_UNBALANCED_TRANSACTION")]
    UnbalancedTransaction(Currency, Layer, Decimal),
    #[error("TxTemplateRejection - NotFound: code '{0}' not found")]
    #[rejection(code = "CALA_TX_TEMPLATE_COULD_NOT_FIND_BY_CODE")]
    CouldNotFindByCode(String),
    #[error("{0}")]
    #[rejection(delegate, from)]
    ParamRejection(crate::param::error::ParamRejection),
}
