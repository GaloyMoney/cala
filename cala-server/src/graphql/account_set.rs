use async_graphql::{dataloader::*, *};

use cala_ledger::{
    balance::*,
    primitives::{AccountId, Currency, JournalId},
};

use super::{balance::Balance, convert::ToGlobalId, loader::LedgerDataLoader, primitives::*};

#[derive(Clone, SimpleObject)]
#[graphql(complex)]
pub struct AccountSet {
    id: ID,
    account_set_id: UUID,
    version: u32,
    journal_id: UUID,
    name: String,
    normal_balance_type: DebitOrCredit,
    description: Option<String>,
    metadata: Option<JSON>,
    created_at: Timestamp,
    modified_at: Timestamp,
}

#[ComplexObject]
impl AccountSet {
    async fn balance(
        &self,
        ctx: &Context<'_>,
        currency: CurrencyCode,
    ) -> async_graphql::Result<Option<Balance>> {
        let loader = ctx.data_unchecked::<DataLoader<LedgerDataLoader>>();
        let journal_id = JournalId::from(self.journal_id);
        let account_id = AccountId::from(self.account_set_id);
        let currency = Currency::from(currency);
        let balance: Option<AccountBalance> =
            loader.load_one((journal_id, account_id, currency)).await?;
        Ok(balance.map(Balance::from))
    }
}

#[derive(InputObject)]
pub(super) struct AccountSetCreateInput {
    pub account_set_id: UUID,
    pub journal_id: UUID,
    pub name: String,
    #[graphql(default)]
    pub normal_balance_type: DebitOrCredit,
    pub description: Option<String>,
    pub metadata: Option<JSON>,
}

#[derive(SimpleObject)]
pub(super) struct AccountSetCreatePayload {
    pub account_set: AccountSet,
}

impl ToGlobalId for cala_ledger::AccountSetId {
    fn to_global_id(&self) -> async_graphql::types::ID {
        async_graphql::types::ID::from(format!("account_set:{}", self))
    }
}

impl From<cala_ledger::account_set::AccountSet> for AccountSet {
    fn from(account_set: cala_ledger::account_set::AccountSet) -> Self {
        let created_at = account_set.created_at();
        let modified_at = account_set.modified_at();
        let values = account_set.into_values();
        Self {
            id: values.id.to_global_id(),
            account_set_id: UUID::from(values.id),
            version: values.version,
            journal_id: UUID::from(values.journal_id),
            name: values.name,
            normal_balance_type: DebitOrCredit::from(values.normal_balance_type),
            description: values.description,
            metadata: values.metadata.map(JSON::from),
            created_at: created_at.into(),
            modified_at: modified_at.into(),
        }
    }
}

impl From<cala_ledger::account_set::AccountSet> for AccountSetCreatePayload {
    fn from(value: cala_ledger::account_set::AccountSet) -> Self {
        Self {
            account_set: AccountSet::from(value),
        }
    }
}
