use async_graphql::{types::connection::*, *};

use super::{account::*, journal::*};
use crate::app::CalaApp;

// use timestamp::*;

// use crate::app::CalaApp;

pub struct Query;

#[Object]
impl Query {
    async fn accounts(
        &self,
        ctx: &Context<'_>,
        first: i32,
        after: Option<String>,
    ) -> Result<Connection<AccountByNameCursor, Account, EmptyFields, EmptyFields>> {
        let app = ctx.data_unchecked::<CalaApp>();
        query(
            after,
            None,
            Some(first),
            None,
            |after, _, first, _| async move {
                let first = first.expect("First always exists");
                let result = app
                    .ledger()
                    .accounts()
                    .list(cala_types::query::PaginatedQueryArgs {
                        first,
                        after: after.map(cala_types::query::AccountByNameCursor::from),
                    })
                    .await?;
                let mut connection = Connection::new(false, result.has_next_page);
                connection
                    .edges
                    .extend(result.entities.into_iter().map(|entity| {
                        let cursor = AccountByNameCursor::from(entity.values());
                        Edge::new(cursor, Account::from(entity.into_values()))
                    }));
                Ok::<_, async_graphql::Error>(connection)
            },
        )
        .await
    }
}

pub struct Mutation;

#[Object]
impl Mutation {
    async fn account_create(
        &self,
        ctx: &Context<'_>,
        input: AccountCreateInput,
    ) -> Result<AccountCreatePayload> {
        let app = ctx.data_unchecked::<CalaApp>();
        let id = if let Some(id) = input.id {
            id.into()
        } else {
            cala_ledger::AccountId::new()
        };
        let mut builder = cala_ledger::account::NewAccount::builder();
        builder
            .id(id)
            .name(input.name)
            .code(input.code)
            .normal_balance_type(input.normal_balance_type.into())
            .status(input.status.into())
            .tags(input.tags.into_iter().map(String::from).collect())
            .metadata(input.metadata)?;
        if let Some(external_id) = input.external_id {
            builder.external_id(external_id);
        }
        if let Some(description) = input.description {
            builder.description(description);
        }
        let account = app.ledger().accounts().create(builder.build()?).await?;
        Ok(account.into_values().into())
    }

    async fn journal_create(
        &self,
        ctx: &Context<'_>,
        input: JournalCreateInput,
    ) -> Result<JournalCreatePayload> {
        let app = ctx.data_unchecked::<CalaApp>();
        let id = if let Some(id) = input.id {
            id.into()
        } else {
            cala_ledger::JournalId::new()
        };
        let mut builder = cala_ledger::journal::NewJournal::builder();
        builder.id(id).name(input.name).status(input.status.into());
        if let Some(external_id) = input.external_id {
            builder.external_id(external_id);
        }
        if let Some(description) = input.description {
            builder.description(description);
        }
        let journal = app.ledger().journals().create(builder.build()?).await?;
        Ok(journal.into_values().into())
    }
}
