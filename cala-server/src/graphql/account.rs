use async_graphql::{dataloader::*, types::connection::*, *};

use cala_ledger::{
    balance::*,
    entry::EntriesByCreatedAtCursor,
    primitives::{AccountId, Currency, JournalId},
};

pub use cala_ledger::account::AccountsByNameCursor;

use crate::app::CalaApp;

use super::{
    account_set::*, balance::Balance, convert::ToGlobalId, entry::Entry, loader::LedgerDataLoader,
    primitives::*,
};

#[derive(Clone, SimpleObject)]
#[graphql(complex)]
pub struct Account {
    id: ID,
    account_id: UUID,
    version: u32,
    code: String,
    name: String,
    normal_balance_type: DebitOrCredit,
    status: Status,
    external_id: Option<String>,
    description: Option<String>,
    metadata: Option<JSON>,
    pub(super) created_at: Timestamp,
    modified_at: Timestamp,
}

#[ComplexObject]
impl Account {
    async fn balance(
        &self,
        ctx: &Context<'_>,
        journal_id: UUID,
        currency: CurrencyCode,
    ) -> async_graphql::Result<Option<Balance>> {
        let journal_id = JournalId::from(journal_id);
        let account_id = AccountId::from(self.account_id);
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
    ) -> Result<Connection<AccountSetsByNameCursor, AccountSet, EmptyFields, EmptyFields>> {
        let app = ctx.data_unchecked::<CalaApp>();
        let account_id = AccountId::from(self.account_id);
        query(
            after,
            None,
            Some(first),
            None,
            |after, _, first, _| async move {
                let first = first.expect("First always exists");
                let query_args = cala_ledger::es_entity::PaginatedQueryArgs { first, after };

                let result = match ctx.data_opt::<DbOp>() {
                    Some(op) => {
                        let mut op = op.try_lock().expect("Lock held concurrently");
                        app.ledger()
                            .account_sets()
                            .find_where_member_in_op(&mut *op, account_id, query_args)
                            .await?
                    }
                    None => {
                        app.ledger()
                            .account_sets()
                            .find_where_member(account_id, query_args)
                            .await?
                    }
                };
                let mut connection = Connection::new(false, result.has_next_page);
                connection
                    .edges
                    .extend(result.entities.into_iter().map(|entity| {
                        let cursor = AccountSetsByNameCursor::from(&entity);
                        Edge::new(cursor, AccountSet::from(entity))
                    }));
                Ok::<_, async_graphql::Error>(connection)
            },
        )
        .await
    }

    async fn entries(
        &self,
        ctx: &Context<'_>,
        first: i32,
        after: Option<String>,
    ) -> Result<Connection<EntriesByCreatedAtCursor, Entry, EmptyFields, EmptyFields>> {
        let app = ctx.data_unchecked::<CalaApp>();
        let account_id = AccountId::from(self.account_id);
        query(
            after,
            None,
            Some(first),
            None,
            |after, _, first, _| async move {
                let first = first.expect("First always exists");
                let result = app
                    .ledger()
                    .entries()
                    .list_for_account_id(
                        account_id,
                        cala_ledger::es_entity::PaginatedQueryArgs { first, after },
                        cala_ledger::es_entity::ListDirection::Descending,
                    )
                    .await?;
                let mut connection = Connection::new(false, result.has_next_page);
                connection
                    .edges
                    .extend(result.entities.into_iter().map(|entity| {
                        let cursor = EntriesByCreatedAtCursor::from(&entity);
                        Edge::new(cursor, Entry::from(entity))
                    }));
                Ok::<_, async_graphql::Error>(connection)
            },
        )
        .await
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
    pub account_set_ids: Option<Vec<UUID>>,
}

#[derive(SimpleObject)]
pub(super) struct AccountCreatePayload {
    pub account: Account,
}

#[derive(InputObject)]
pub(super) struct AccountUpdateInput {
    pub external_id: Option<String>,
    pub code: Option<String>,
    pub name: Option<String>,
    pub normal_balance_type: Option<DebitOrCredit>,
    pub description: Option<String>,
    pub status: Option<Status>,
    pub metadata: Option<JSON>,
}

#[derive(SimpleObject)]
pub(super) struct AccountUpdatePayload {
    pub account: Account,
}

impl ToGlobalId for cala_ledger::AccountId {
    fn to_global_id(&self) -> async_graphql::types::ID {
        async_graphql::types::ID::from(format!("account:{self}"))
    }
}

impl From<cala_ledger::account::Account> for Account {
    fn from(account: cala_ledger::account::Account) -> Self {
        let created_at = account.created_at();
        let modified_at = account.modified_at();
        let values = account.into_values();
        Self {
            id: values.id.to_global_id(),
            account_id: UUID::from(values.id),
            version: values.version,
            code: values.code,
            name: values.name,
            normal_balance_type: values.normal_balance_type,
            status: values.status,
            external_id: values.external_id,
            description: values.description,
            metadata: values.metadata.map(JSON::from),
            created_at: created_at.into(),
            modified_at: modified_at.into(),
        }
    }
}

impl From<cala_ledger::account::Account> for AccountCreatePayload {
    fn from(value: cala_ledger::account::Account) -> Self {
        Self {
            account: Account::from(value),
        }
    }
}

impl From<cala_ledger::account::Account> for AccountUpdatePayload {
    fn from(value: cala_ledger::account::Account) -> Self {
        Self {
            account: Account::from(value),
        }
    }
}
