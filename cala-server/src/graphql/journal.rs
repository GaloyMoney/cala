use async_graphql::*;

use super::primitives::*;

#[derive(InputObject)]
pub struct JournalCreateInput {
    pub journal_id: UUID,
    pub name: String,
    pub external_id: Option<String>,
    #[graphql(default)]
    pub status: Status,
    pub description: Option<String>,
}

#[derive(SimpleObject)]
pub struct Journal {
    pub id: ID,
    pub journal_id: UUID,
    pub name: String,
    pub external_id: Option<String>,
    pub status: Status,
    pub description: Option<String>,
}

#[derive(SimpleObject)]
pub struct JournalCreatePayload {
    pub journal: Journal,
}
