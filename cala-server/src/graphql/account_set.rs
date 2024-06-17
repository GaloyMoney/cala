use async_graphql::{dataloader::*, types::connection::*, *};
use serde::{Deserialize, Serialize};

use cala_ledger::{
    account_set::AccountSetMember,
    balance::*,
    primitives::{AccountId, AccountSetId, Currency, JournalId},
};

use super::{
    balance::Balance, convert::ToGlobalId, loader::LedgerDataLoader, primitives::*, schema::DbOp,
};
use crate::app::CalaApp;

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
        let journal_id = JournalId::from(self.journal_id);
        let account_id = AccountId::from(self.account_set_id);
        let currency = Currency::from(currency);

        let balance: Option<AccountBalance> = match ctx.data_opt::<DbOp>() {
            Some(op) => {
                let app = ctx.data_unchecked::<CalaApp>();
                let mut op = op.try_lock().expect("Lock held concurrently");
                Some(
                    app.ledger()
                        .balances()
                        .find_in_op(&mut op, journal_id, account_id, currency)
                        .await?,
                )
            }
            None => {
                let loader = ctx.data_unchecked::<DataLoader<LedgerDataLoader>>();
                loader.load_one((journal_id, account_id, currency)).await?
            }
        };
        Ok(balance.map(Balance::from))
    }

    async fn sets(
        &self,
        ctx: &Context<'_>,
        first: i32,
        after: Option<String>,
    ) -> Result<Connection<AccountSetByNameCursor, AccountSet, EmptyFields, EmptyFields>> {
        let app = ctx.data_unchecked::<CalaApp>();
        let account_set_id = AccountSetId::from(self.account_set_id);

        query(
            after.clone(),
            None,
            Some(first),
            None,
            |after, _, first, _| async move {
                let first = first.expect("First always exists");
                let query_args = cala_ledger::query::PaginatedQueryArgs {
                    first,
                    after: after.map(cala_ledger::account_set::AccountSetByNameCursor::from),
                };

                let result = match ctx.data_opt::<DbOp>() {
                    Some(op) => {
                        let mut op = op.try_lock().expect("Lock held concurrently");
                        app.ledger()
                            .account_sets()
                            .find_where_member_in_op(&mut op, account_set_id, query_args)
                            .await?
                    }
                    None => {
                        app.ledger()
                            .account_sets()
                            .find_where_member(account_set_id, query_args)
                            .await?
                    }
                };

                let mut connection = Connection::new(false, result.has_next_page);
                connection
                    .edges
                    .extend(result.entities.into_iter().map(|entity| {
                        let cursor = AccountSetByNameCursor::from(entity.values());
                        Edge::new(cursor, AccountSet::from(entity))
                    }));

                Ok::<_, async_graphql::Error>(connection)
            },
        )
        .await
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

#[derive(Enum, Copy, Clone, Eq, PartialEq)]
pub enum AccountSetMemberType {
    Account,
    AccountSet,
}

#[derive(InputObject)]
pub(super) struct AddToAccountSetInput {
    pub account_set_id: UUID,
    pub member_id: UUID,
    pub member_type: AccountSetMemberType,
}

impl From<AddToAccountSetInput> for AccountSetMember {
    fn from(input: AddToAccountSetInput) -> Self {
        match input.member_type {
            AccountSetMemberType::Account => {
                AccountSetMember::Account(AccountId::from(input.member_id))
            }
            AccountSetMemberType::AccountSet => {
                AccountSetMember::AccountSet(AccountSetId::from(input.member_id))
            }
        }
    }
}

#[derive(SimpleObject)]
pub(super) struct AddToAccountSetPayload {
    pub account_set: AccountSet,
}

#[derive(InputObject)]
pub(super) struct RemoveFromAccountSetInput {
    pub account_set_id: UUID,
    pub member_id: UUID,
    pub member_type: AccountSetMemberType,
}

impl From<RemoveFromAccountSetInput> for AccountSetMember {
    fn from(input: RemoveFromAccountSetInput) -> Self {
        match input.member_type {
            AccountSetMemberType::Account => {
                AccountSetMember::Account(AccountId::from(input.member_id))
            }
            AccountSetMemberType::AccountSet => {
                AccountSetMember::AccountSet(AccountSetId::from(input.member_id))
            }
        }
    }
}

#[derive(SimpleObject)]
pub(super) struct RemoveFromAccountSetPayload {
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

impl From<cala_ledger::account_set::AccountSet> for AddToAccountSetPayload {
    fn from(value: cala_ledger::account_set::AccountSet) -> Self {
        Self {
            account_set: AccountSet::from(value),
        }
    }
}

impl From<cala_ledger::account_set::AccountSet> for RemoveFromAccountSetPayload {
    fn from(value: cala_ledger::account_set::AccountSet) -> Self {
        Self {
            account_set: AccountSet::from(value),
        }
    }
}

impl From<&cala_ledger::account_set::AccountSetValues> for AccountSetByNameCursor {
    fn from(values: &cala_ledger::account_set::AccountSetValues) -> Self {
        Self {
            name: values.name.clone(),
            id: values.id,
        }
    }
}

#[derive(InputObject)]
pub(super) struct AccountSetUpdateInput {
    pub name: Option<String>,
    pub normal_balance_type: Option<DebitOrCredit>,
    pub description: Option<String>,
    pub metadata: Option<JSON>,
}

#[derive(SimpleObject)]
pub(super) struct AccountSetUpdatePayload {
    pub account_set: AccountSet,
}

impl From<cala_ledger::account_set::AccountSet> for AccountSetUpdatePayload {
    fn from(value: cala_ledger::account_set::AccountSet) -> Self {
        Self {
            account_set: AccountSet::from(value),
        }
    }
}

#[derive(Serialize, Deserialize)]
pub(super) struct AccountSetByNameCursor {
    pub name: String,
    pub id: cala_ledger::primitives::AccountSetId,
}

impl CursorType for AccountSetByNameCursor {
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

impl From<AccountSetByNameCursor> for cala_ledger::account_set::AccountSetByNameCursor {
    fn from(cursor: AccountSetByNameCursor) -> Self {
        Self {
            name: cursor.name,
            id: cursor.id,
        }
    }
}
