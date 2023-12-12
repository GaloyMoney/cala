use async_graphql::*;

use super::primitives::*;

#[derive(SimpleObject)]
pub(super) struct Account {
    pub id: ID,
    pub code: String,
    pub name: String,
    pub external_id: String,
    pub normal_balance_type: DebitOrCredit,
    pub status: Status,
    pub description: String,
    pub tags: Vec<String>,
    pub metadata: Option<JSON>,
}
