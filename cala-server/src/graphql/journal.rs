use async_graphql::*;

use super::{convert::ToGlobalId, primitives::*};

#[derive(InputObject)]
pub struct JournalCreateInput {
    pub journal_id: UUID,
    pub name: String,
    #[graphql(default)]
    pub status: Status,
    pub description: Option<String>,
}

#[derive(SimpleObject)]
pub struct Journal {
    pub id: ID,
    pub journal_id: UUID,
    pub version: u32,
    pub name: String,
    pub external_id: Option<String>,
    pub status: Status,
    pub description: Option<String>,
}

#[derive(SimpleObject)]
pub struct JournalCreatePayload {
    pub journal: Journal,
}

impl ToGlobalId for cala_ledger::JournalId {
    fn to_global_id(&self) -> async_graphql::types::ID {
        async_graphql::types::ID::from(format!("journal:{}", self))
    }
}

impl From<cala_ledger::journal::JournalValues> for Journal {
    fn from(value: cala_ledger::journal::JournalValues) -> Self {
        Self {
            id: value.id.to_global_id(),
            journal_id: UUID::from(value.id),
            version: value.version,
            name: value.name,
            external_id: value.external_id,
            status: Status::from(value.status),
            description: value.description,
        }
    }
}
