use async_graphql::{dataloader::*, types::connection::*, *};
use serde::{Deserialize, Serialize};

use cala_ledger::{
    balance::*,
    primitives::{AccountId, Currency, JournalId},
};

use super::{balance::Balance, convert::ToGlobalId, loader::LedgerDataLoader, primitives::*};

#[derive(SimpleObject)]
#[graphql(complex)]
pub(super) struct Account {
    pub id: ID,
    pub account_id: UUID,
    pub code: String,
    pub name: String,
    pub normal_balance_type: DebitOrCredit,
    pub status: Status,
    pub external_id: Option<String>,
    pub description: Option<String>,
    pub metadata: Option<JSON>,
}

#[ComplexObject]
impl Account {
    async fn balance(
        &self,
        ctx: &Context<'_>,
        journal_id: UUID,
        currency: CurrencyCode,
    ) -> async_graphql::Result<Option<Balance>> {
        let loader = ctx.data_unchecked::<DataLoader<LedgerDataLoader>>();
        let journal_id = JournalId::from(journal_id);
        let account_id = AccountId::from(self.account_id);
        let currency = Currency::from(currency);
        let balance: Option<AccountBalance> =
            loader.load_one((journal_id, account_id, currency)).await?;
        Ok(balance.map(Balance::from))
    }
}

#[derive(Serialize, Deserialize)]
pub(super) struct AccountByNameCursor {
    pub name: String,
    pub id: cala_ledger::primitives::AccountId,
}

impl CursorType for AccountByNameCursor {
    type Error = String;

    fn encode_cursor(&self) -> String {
        use base64::{engine::general_purpose, Engine as _};
        let json = serde_json::to_string(&self).expect("could not serialize token");
        general_purpose::STANDARD_NO_PAD.encode(json.as_bytes())
    }

    fn decode_cursor(s: &str) -> Result<Self, Self::Error> {
        use base64::{engine::general_purpose, Engine as _};
        let bytes = general_purpose::STANDARD_NO_PAD
            .decode(s.as_bytes())
            .map_err(|e| e.to_string())?;
        let json = String::from_utf8(bytes).map_err(|e| e.to_string())?;
        serde_json::from_str(&json).map_err(|e| e.to_string())
    }
}

#[derive(InputObject)]
pub(super) struct AccountCreateInput {
    pub account_id: UUID,
    pub external_id: Option<String>,
    pub code: String,
    pub name: String,
    #[graphql(default)]
    pub normal_balance_type: DebitOrCredit,
    pub description: Option<String>,
    #[graphql(default)]
    pub status: Status,
    pub metadata: Option<JSON>,
}

#[derive(SimpleObject)]
pub(super) struct AccountCreatePayload {
    pub account: Account,
}

impl ToGlobalId for cala_ledger::AccountId {
    fn to_global_id(&self) -> async_graphql::types::ID {
        async_graphql::types::ID::from(format!("account:{}", self))
    }
}

impl From<&cala_ledger::account::AccountValues> for AccountByNameCursor {
    fn from(values: &cala_ledger::account::AccountValues) -> Self {
        Self {
            name: values.name.clone(),
            id: values.id,
        }
    }
}

impl From<AccountByNameCursor> for cala_ledger::account::AccountByNameCursor {
    fn from(cursor: AccountByNameCursor) -> Self {
        Self {
            name: cursor.name,
            id: cursor.id,
        }
    }
}

impl From<cala_ledger::account::Account> for Account {
    fn from(account: cala_ledger::account::Account) -> Self {
        Self::from(account.into_values())
    }
}
impl From<cala_ledger::account::AccountValues> for Account {
    fn from(values: cala_ledger::account::AccountValues) -> Self {
        Self {
            id: values.id.to_global_id(),
            account_id: UUID::from(values.id),
            code: values.code,
            name: values.name,
            normal_balance_type: DebitOrCredit::from(values.normal_balance_type),
            status: Status::from(values.status),
            external_id: values.external_id,
            description: values.description,
            metadata: values.metadata.map(JSON::from),
        }
    }
}
