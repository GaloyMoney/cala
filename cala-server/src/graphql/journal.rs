use async_graphql::*;

use super::{convert::ToGlobalId, primitives::*};

#[derive(InputObject)]
pub struct JournalCreateInput {
    pub(super) journal_id: UUID,
    pub(super) name: String,
    #[graphql(default)]
    pub(super) status: Status,
    pub(super) description: Option<String>,
}

#[derive(Clone, SimpleObject)]
pub struct Journal {
    id: ID,
    journal_id: UUID,
    version: u32,
    name: String,
    status: Status,
    description: Option<String>,
    created_at: Timestamp,
    modified_at: Timestamp,
}

#[derive(SimpleObject)]
pub struct JournalCreatePayload {
    pub journal: Journal,
}

impl ToGlobalId for cala_ledger::JournalId {
    fn to_global_id(&self) -> async_graphql::types::ID {
        async_graphql::types::ID::from(format!("journal:{self}"))
    }
}

impl From<cala_ledger::journal::Journal> for Journal {
    fn from(entity: cala_ledger::journal::Journal) -> Self {
        let created_at = entity.created_at();
        let modified_at = entity.modified_at();
        let values = entity.into_values();
        Self {
            id: values.id.to_global_id(),
            journal_id: UUID::from(values.id),
            version: values.version,
            name: values.name,
            status: values.status,
            description: values.description,
            created_at: Timestamp::from(created_at),
            modified_at: Timestamp::from(modified_at),
        }
    }
}

impl From<cala_ledger::journal::Journal> for JournalCreatePayload {
    fn from(value: cala_ledger::journal::Journal) -> Self {
        JournalCreatePayload {
            journal: Journal::from(value),
        }
    }
}

#[derive(InputObject)]
pub struct JournalUpdateInput {
    pub(super) name: Option<String>,
    pub(super) status: Option<Status>,
    pub(super) description: Option<String>,
}

#[derive(SimpleObject)]
pub struct JournalUpdatePayload {
    pub journal: Journal,
}

impl From<cala_ledger::journal::Journal> for JournalUpdatePayload {
    fn from(value: cala_ledger::journal::Journal) -> Self {
        JournalUpdatePayload {
            journal: Journal::from(value),
        }
    }
}
