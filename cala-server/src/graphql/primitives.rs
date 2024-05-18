#![allow(clippy::upper_case_acronyms)]
use async_graphql::*;
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

#[derive(Enum, Copy, Clone, Eq, PartialEq)]
#[graphql(remote = "cala_ledger::primitives::DebitOrCredit")]
pub(super) enum DebitOrCredit {
    Debit,
    Credit,
}

impl Default for DebitOrCredit {
    fn default() -> Self {
        Self::Credit
    }
}

#[derive(Enum, Copy, Clone, Eq, PartialEq)]
#[graphql(remote = "cala_ledger::primitives::Status")]
pub(super) enum Status {
    Active,
    Locked,
}

impl Default for Status {
    fn default() -> Self {
        Self::Active
    }
}

#[derive(Serialize, Deserialize)]
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

#[derive(Serialize, Deserialize)]
#[serde(transparent)]
pub struct UUID(uuid::Uuid);
scalar!(UUID);
impl<T: Into<uuid::Uuid>> From<T> for UUID {
    fn from(id: T) -> Self {
        let uuid = id.into();
        Self(uuid)
    }
}

impl From<UUID> for cala_ledger::AccountId {
    fn from(uuid: UUID) -> Self {
        cala_ledger::AccountId::from(uuid.0)
    }
}

impl From<UUID> for cala_ledger::JournalId {
    fn from(uuid: UUID) -> Self {
        cala_ledger::JournalId::from(uuid.0)
    }
}

impl From<UUID> for cala_ledger::TxTemplateId {
    fn from(uuid: UUID) -> Self {
        cala_ledger::TxTemplateId::from(uuid.0)
    }
}

impl From<UUID> for cala_ledger::TransactionId {
    fn from(uuid: UUID) -> Self {
        cala_ledger::TransactionId::from(uuid.0)
    }
}

#[derive(Serialize, Deserialize)]
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

#[derive(Serialize, Deserialize)]
#[serde(transparent)]
pub struct Date(NaiveDate);
scalar!(Date);
impl From<NaiveDate> for Date {
    fn from(value: NaiveDate) -> Self {
        Self(value)
    }
}

#[derive(Serialize, Deserialize)]
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

#[derive(Serialize, Deserialize)]
#[serde(transparent)]
pub struct Decimal(rust_decimal::Decimal);
scalar!(Decimal);
impl From<rust_decimal::Decimal> for Decimal {
    fn from(value: rust_decimal::Decimal) -> Self {
        Self(value)
    }
}
