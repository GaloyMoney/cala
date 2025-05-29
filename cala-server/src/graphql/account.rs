use async_graphql::{dataloader::*, types::connection::*, *};

use cala_ledger::{
    balance::*,
    entry::EntriesByCreatedAtCursor,
    primitives::{AccountId, Currency, JournalId},
};

pub use cala_ledger::account::AccountsByNameCursor;

use crate::app::CalaApp;

use super::{
    account_set::*, balance::Balance, convert::ToGlobalId, loader::LedgerDataLoader, primitives::*,
    schema::DbOp,
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

/// A transaction event for an account, representing deposits, withdrawals, trades, etc.
#[derive(SimpleObject, Clone)]
pub struct AccountTransactionEvent {
    /// Event UUID
    pub id: UUID,
    /// Event type (e.g., "initialized", "transfer", etc.)
    pub event_type: String,
    /// Amount (as string for precision)
    pub units: String,
    /// Currency code (e.g., "NGN", "BTC")
    pub currency: String,
    /// Direction (e.g., "debit", "credit")
    pub direction: String,
    /// Transaction UUID
    pub transaction_id: UUID,
    /// Event creation time
    pub created_at: Timestamp,
}

#[ComplexObject]
impl Account {
    /// Simple test field that returns a static string
    async fn test_field(&self) -> String {
        "This is a test field".to_string()
    }
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

    /// Paginated transaction events for this account (deposits, withdrawals, trades, etc.)
    async fn transactions(
        &self,
        ctx: &Context<'_>,
        after: Option<String>,
        first: Option<i32>,
    ) -> async_graphql::Result<Connection<String, AccountTransactionEvent>> {
        let account_id = AccountId::from(self.account_id);
        let limit = first.unwrap_or(20).min(100);
        
        // Parse the cursor from the string if provided
        let cursor = after.map(|s| serde_json::from_str::<EntriesByCreatedAtCursor>(&s))
                        .transpose()?;
        
        // Create pagination query args
        let query_args = es_entity::PaginatedQueryArgs {
            first: limit as usize,
            after: cursor,
        };
        
        // Use descending order to get newest transactions first
        let direction = es_entity::ListDirection::Descending;
        
        // Get entries using the existing pattern from balance()
        let entries_result = match ctx.data_opt::<DbOp>() {
            Some(op) => {
                let app = ctx.data_unchecked::<CalaApp>();
                let _op = op.try_lock().expect("Lock held concurrently");
                // We need to implement this method or use an equivalent
                app.ledger()
                    .entries()
                    .list_for_account_id(account_id, query_args, direction)
                    .await?
            }
            None => {
                // If we're outside a transaction, use the app directly
                let app = ctx.data_unchecked::<CalaApp>();
                app.ledger()
                    .entries()
                    .list_for_account_id(account_id, query_args, direction)
                    .await?
            }
        };
        
        // Convert entries to GraphQL connection
        let mut connection = Connection::new(false, entries_result.has_next_page);
        
        // Transform entries into AccountTransactionEvent objects
        for entry in entries_result.entities {
            let entry_values = entry.values();
            
            // Create a transaction event from the entry using actual values from the entry
            let event = AccountTransactionEvent {
                id: UUID::from(entry.id()),
                event_type: format!("entry:{}", entry_values.id),
                units: entry_values.units.to_string(), // Use actual units
                currency: entry_values.currency.to_string(), // Use actual currency
                direction: entry_values.direction.to_string().to_lowercase(), // Use actual direction
                transaction_id: UUID::from(entry_values.transaction_id),
                created_at: entry.created_at().into(),
            };
            
            // Use a simple numeric string as the cursor
            let cursor = entry_values.id.to_string(); 
            connection.edges.push(Edge::new(cursor, event));
        }
        
        Ok(connection)
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
                            .find_where_member_in_op(&mut op, account_id, query_args)
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
        async_graphql::types::ID::from(format!("account:{}", self))
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
