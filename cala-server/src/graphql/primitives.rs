use async_graphql::*;
use serde::{Deserialize, Serialize};

#[derive(Enum, Copy, Clone, Eq, PartialEq)]
#[graphql(remote = "cala_types::primitives::DebitOrCredit")]
pub(super) enum DebitOrCredit {
    Debit,
    Credit,
}

impl Default for DebitOrCredit {
    fn default() -> Self {
        Self::Debit
    }
}

#[derive(Enum, Copy, Clone, Eq, PartialEq)]
#[graphql(remote = "cala_types::primitives::Status")]
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

impl From<TAG> for String {
    fn from(tag: TAG) -> Self {
        tag.0
    }
}

impl From<UUID> for cala_ledger::JournalId {
    fn from(uuid: UUID) -> Self {
        cala_ledger::JournalId::from(uuid.0)
    }
}

impl From<UUID> for cala_ledger::AccountId {
    fn from(uuid: UUID) -> Self {
        cala_ledger::AccountId::from(uuid.0)
    }
}
