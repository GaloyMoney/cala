use async_graphql::*;

use super::primitives::*;

#[derive(SimpleObject)]
pub(super) struct Account {
    pub id: ID,
    pub account_id: UUID,
    pub code: String,
    pub name: String,
    pub normal_balance_type: DebitOrCredit,
    pub status: Status,
    pub external_id: Option<String>,
    pub description: Option<String>,
    pub tags: Vec<TAG>,
    pub metadata: Option<JSON>,
}
