use serde::{Deserialize, Serialize};
use uuid::Uuid;

crate::entity_id! { OutboxEventId }
crate::entity_id! { AccountId }
crate::entity_id! { JournalId }
crate::entity_id! { DataSourceId }

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq, sqlx::Type)]
#[sqlx(type_name = "DebitOrCredit", rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum DebitOrCredit {
    Debit,
    Credit,
}

impl Default for DebitOrCredit {
    fn default() -> Self {
        Self::Credit
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq, sqlx::Type)]
#[sqlx(type_name = "Status", rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Active,
    Locked,
}

impl Default for Status {
    fn default() -> Self {
        Self::Active
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, sqlx::Type)]
#[sqlx(transparent)]
pub struct Tag(String);

impl Tag {
    pub fn into_inner(self) -> String {
        self.0
    }
}

#[derive(thiserror::Error, Debug)]
pub enum ParseTagError {
    #[error("Tag must be 64 characters or less.")]
    TooLong,
    #[error("Tag must not contain spaces.")]
    ContainsSpace,
}

impl std::str::FromStr for Tag {
    type Err = ParseTagError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.len() >= 64 {
            Err(ParseTagError::TooLong)
        } else if s.contains(' ') {
            Err(ParseTagError::ContainsSpace)
        } else {
            Ok(Tag(s.to_string()))
        }
    }
}

const _LOCAL_UUID: Uuid = uuid::uuid!("00000000-0000-0000-0000-000000000000");

#[derive(Debug, Copy, Serialize, Deserialize, Clone)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DataSource {
    Local,
    Remote { id: DataSourceId },
}

impl std::fmt::Display for DataSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DataSource::Local => write!(f, "00000000-0000-0000-0000-000000000000"),
            DataSource::Remote { id } => write!(f, "{}", id),
        }
    }
}

impl std::str::FromStr for DataSource {
    type Err = uuid::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s == "00000000-0000-0000-0000-000000000000" {
            Ok(DataSource::Local)
        } else {
            Ok(DataSource::Remote { id: s.parse()? })
        }
    }
}
