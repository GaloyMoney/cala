use async_graphql::*;

use super::primitives::*;

#[derive(InputObject)]
pub struct TransactionInput {
    pub transaction_id: UUID,
    pub tx_template_code: String,
    pub params: Option<JSON>,
}

#[derive(SimpleObject)]
pub struct Transaction {
    pub id: ID,
    pub transaction_id: UUID,
    pub tx_template_id: UUID,
    pub journal_id: UUID,
    pub effective: Date,
    pub correlation_id: String,
    pub external_id: Option<String>,
    pub description: Option<String>,
    pub metadata: Option<JSON>,
}

#[derive(SimpleObject)]
pub struct PostTransactionPayload {
    pub transaction: Transaction,
}
