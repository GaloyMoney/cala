use async_graphql::{dataloader::*, *};

use cala_ledger::primitives::{AccountId, DebitOrCredit, Layer, TransactionId};

use super::{
    account::Account, convert::ToGlobalId, loader::LedgerDataLoader, primitives::*,
    transaction::Transaction,
};

#[derive(Clone, SimpleObject)]
#[graphql(complex)]
pub struct Entry {
    id: ID,
    entry_id: UUID,
    version: u32,
    transaction_id: UUID,
    journal_id: UUID,
    account_id: UUID,
    entry_type: String,
    sequence: u32,
    layer: Layer,
    units: Decimal,
    direction: DebitOrCredit,
    description: Option<String>,
    created_at: Timestamp,
}

#[ComplexObject]
impl Entry {
    async fn account(&self, ctx: &Context<'_>) -> async_graphql::Result<Account> {
        let loader = ctx.data_unchecked::<DataLoader<LedgerDataLoader>>();
        Ok(loader
            .load_one(AccountId::from(self.account_id))
            .await?
            .expect("Account not found"))
    }

    async fn transaction(&self, ctx: &Context<'_>) -> async_graphql::Result<Transaction> {
        let loader = ctx.data_unchecked::<DataLoader<LedgerDataLoader>>();
        Ok(loader
            .load_one(TransactionId::from(self.transaction_id))
            .await?
            .expect("transaction not found"))
    }
}

impl ToGlobalId for cala_ledger::EntryId {
    fn to_global_id(&self) -> async_graphql::types::ID {
        use base64::{engine::general_purpose, Engine as _};
        let id = format!(
            "entry:{}",
            general_purpose::STANDARD_NO_PAD.encode(self.to_string())
        );
        async_graphql::types::ID::from(id)
    }
}

impl From<cala_ledger::entry::Entry> for Entry {
    fn from(entity: cala_ledger::entry::Entry) -> Self {
        let created_at = entity.created_at();
        let values = entity.into_values();
        Self {
            id: values.id.to_global_id(),
            entry_id: UUID::from(values.id),
            version: values.version,
            transaction_id: UUID::from(values.transaction_id),
            account_id: UUID::from(values.account_id),
            journal_id: UUID::from(values.journal_id),
            entry_type: values.entry_type,
            sequence: values.sequence,
            layer: values.layer,
            units: Decimal::from(values.units),
            direction: values.direction,
            description: values.description,
            created_at: Timestamp::from(created_at),
        }
    }
}
