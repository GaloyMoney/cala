use async_graphql::*;

use super::primitives::*;

#[derive(InputObject)]
pub struct CreateJournalInput {
    pub id: Option<UUID>,
    pub name: String,
    pub external_id: Option<String>,
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
