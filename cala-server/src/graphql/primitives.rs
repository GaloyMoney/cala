#![allow(clippy::upper_case_acronyms)]
use async_graphql::*;
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

pub use cala_ledger::primitives::{DebitOrCredit, Layer, Status};

use std::sync::Arc;
use tokio::sync::Mutex;
pub type DbOp = Arc<Mutex<cala_ledger::LedgerOperation<'static>>>;

pub use es_entity::graphql::UUID;

#[derive(Clone, Serialize, Deserialize)]
#[serde(transparent)]
pub struct JSON(serde_json::Value);
scalar!(JSON);
impl From<serde_json::Value> for JSON {
    fn from(value: serde_json::Value) -> Self {
        Self(value)
    }
}

impl JSON {
    pub fn into_inner(self) -> serde_json::Value {
        self.0
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Timestamp(chrono::DateTime<chrono::Utc>);
scalar!(Timestamp);
impl Timestamp {
    pub fn into_inner(self) -> chrono::DateTime<chrono::Utc> {
        self.0
    }
}
impl From<chrono::DateTime<chrono::Utc>> for Timestamp {
    fn from(value: chrono::DateTime<chrono::Utc>) -> Self {
        Self(value)
    }
}

#[derive(Enum, Copy, Clone, PartialEq, Eq)]
#[graphql(remote = "cala_ledger::tx_template::ParamDataType")]
pub enum ParamDataType {
    String,
    Integer,
    Decimal,
    Boolean,
    Uuid,
    Date,
    Timestamp,
    Json,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Expression(String);
scalar!(Expression);

impl From<cel_interpreter::CelExpression> for Expression {
    fn from(expr: cel_interpreter::CelExpression) -> Self {
        Self(expr.to_string())
    }
}

impl From<Expression> for String {
    fn from(expr: Expression) -> Self {
        expr.0
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Date(NaiveDate);
scalar!(Date);
impl From<NaiveDate> for Date {
    fn from(value: NaiveDate) -> Self {
        Self(value)
    }
}
impl From<Date> for NaiveDate {
    fn from(value: Date) -> Self {
        value.0
    }
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(transparent)]
pub struct CurrencyCode(cala_types::primitives::Currency);
scalar!(CurrencyCode);
impl From<CurrencyCode> for cala_types::primitives::Currency {
    fn from(code: CurrencyCode) -> Self {
        code.0
    }
}
impl From<cala_types::primitives::Currency> for CurrencyCode {
    fn from(code: cala_types::primitives::Currency) -> Self {
        Self(code)
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Decimal(rust_decimal::Decimal);
scalar!(Decimal);
impl From<rust_decimal::Decimal> for Decimal {
    fn from(value: rust_decimal::Decimal) -> Self {
        Self(value)
    }
}
impl From<Decimal> for rust_decimal::Decimal {
    fn from(value: Decimal) -> Self {
        value.0
    }
}
