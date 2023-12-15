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
                        first: usize::try_from(first)?,
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
    async fn create_journal(&self, ctx: &Context<'_>, input: JournalInput) -> Result<Journal> {
        let app = ctx.data_unchecked::<CalaApp>();
        let id = if let Some(id) = input.id {
            id.into()
        } else {
            cala_ledger::JournalId::new()
        };
        let mut new = cala_ledger::journal::NewJournal::builder();
        new.id(id).name(input.name);
        if let Some(external_id) = input.external_id {
            new.external_id(external_id);
        }
        if let Some(description) = input.description {
            new.description(description);
        }
        let journal = app.ledger().journals().create(new.build()?).await?;
        Ok(journal.into_values().into())
    }
}
