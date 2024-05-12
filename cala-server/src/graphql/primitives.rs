#![allow(clippy::upper_case_acronyms)]
use async_graphql::*;
use serde::{Deserialize, Serialize};

#[derive(Enum, Copy, Clone, Eq, PartialEq)]
#[graphql(remote = "cala_ledger::primitives::DebitOrCredit")]
pub(super) enum DebitOrCredit {
    Debit,
    Credit,
}

#[derive(Enum, Copy, Clone, Eq, PartialEq)]
#[graphql(remote = "cala_ledger::primitives::Status")]
pub(super) enum Status {
    Active,
    Locked,
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

impl Default for Status {
    fn default() -> Self {
        Self::Active
    }
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

#[derive(Serialize, Deserialize)]
#[serde(transparent)]
pub struct TAG(String);
scalar!(TAG);
impl From<String> for TAG {
    fn from(tag: String) -> Self {
        Self(tag)
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

#[derive(Serialize, Deserialize)]
#[serde(transparent)]
pub struct Expression(String);
scalar!(Expression);

impl From<cel_interpreter::CelExpression> for Expression {
    fn from(expr: cel_interpreter::CelExpression) -> Self {
        Self(expr.to_string())
    }
}
impl AsRef<str> for Expression {
    fn as_ref(&self) -> &str {
        &self.0
    }
}
