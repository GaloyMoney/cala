use async_graphql::{types::connection::*, *};

use super::{account::*, import_job::*, journal::*};
use crate::{app::CalaApp, extension::MutationExtensionMarker};

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
                    .list(cala_ledger::query::PaginatedQueryArgs {
                        first,
                        after: after.map(cala_ledger::account::AccountByNameCursor::from),
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

    async fn import_jobs(
        &self,
        ctx: &Context<'_>,
        first: i32,
        after: Option<String>,
    ) -> Result<Connection<ImportJobByNameCursor, ImportJob, EmptyFields, EmptyFields>> {
        let app = ctx.data_unchecked::<CalaApp>();
        query(
            after,
            None,
            Some(first),
            None,
            |after, _, first, _| async move {
                let first = first.expect("First always exists");
                let result = app
                    .list_import_jobs(cala_ledger::query::PaginatedQueryArgs {
                        first,
                        after: after.map(crate::import_job::ImportJobByNameCursor::from),
                    })
                    .await?;
                let mut connection = Connection::new(false, result.has_next_page);
                connection
                    .edges
                    .extend(result.entities.into_iter().map(|entity| {
                        let cursor = ImportJobByNameCursor::from(&entity);
                        Edge::new(cursor, ImportJob::from(entity))
                    }));
                Ok::<_, async_graphql::Error>(connection)
            },
        )
        .await
    }
}

#[derive(Default)]
pub struct CoreMutation<E: MutationExtensionMarker> {
    _phantom: std::marker::PhantomData<E>,
}

#[Object(name = "Mutation")]
impl<E: MutationExtensionMarker> CoreMutation<E> {
    #[graphql(flatten)]
    async fn extension(&self) -> E {
        E::default()
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

    async fn import_job_create(
        &self,
        ctx: &Context<'_>,
        input: ImportJobCreateInput,
    ) -> Result<ImportJobCreatePayload> {
        let app = ctx.data_unchecked::<CalaApp>();
        Ok(ImportJobCreatePayload {
            import_job: app
                .create_import_job(input.name, input.description, input.endpoint)
                .await
                .map(ImportJob::from)?,
        })
    }
}
