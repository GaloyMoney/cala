use async_graphql::*;

use super::{convert::ToGlobalId, primitives::*};

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
    pub version: u32,
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

impl ToGlobalId for cala_ledger::TransactionId {
    fn to_global_id(&self) -> async_graphql::types::ID {
        use base64::{engine::general_purpose, Engine as _};
        let id = format!(
            "transaction:{}",
            general_purpose::STANDARD_NO_PAD.encode(self.to_string())
        );
        async_graphql::types::ID::from(id)
    }
}

impl From<cala_ledger::transaction::TransactionValues> for Transaction {
    fn from(values: cala_ledger::transaction::TransactionValues) -> Self {
        Self {
            id: values.id.to_global_id(),
            transaction_id: UUID::from(values.id),
            version: values.version,
            tx_template_id: UUID::from(values.tx_template_id),
            journal_id: UUID::from(values.journal_id),
            effective: Date::from(values.effective),
            correlation_id: values.correlation_id,
            external_id: values.external_id,
            description: values.description,
            metadata: values.metadata.map(JSON::from),
        }
    }
}
