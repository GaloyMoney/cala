use async_graphql::*;

use super::job::*;
use crate::{
    app::CalaApp,
    graphql::{DbOp, Job},
};

#[derive(InputObject)]
pub struct CalaOutboxImportJobCreateInput {
    pub name: String,
    pub description: Option<String>,
    pub endpoint: String,
}

#[derive(SimpleObject)]
pub struct CalaOutboxImportJobCreatePayload {
    pub job: Job,
}

#[derive(Default)]
pub struct Mutation;

#[Object(name = "CalaOutboxImportMutation")]
impl Mutation {
    async fn cala_outbox_import_job_create(
        &self,
        ctx: &Context<'_>,
        input: CalaOutboxImportJobCreateInput,
    ) -> async_graphql::Result<CalaOutboxImportJobCreatePayload> {
        let app = ctx.data_unchecked::<CalaApp>();
        let mut op = ctx
            .data_unchecked::<DbOp>()
            .try_lock()
            .expect("Lock held concurrently");
        let job = app
            .create_and_spawn_job_in_op::<CalaOutboxImportJobInitializer, _>(
                &mut op,
                input.name.clone(),
                input.description.clone(),
                CalaOutboxImportConfig::from(input),
            )
            .await?;
        Ok(CalaOutboxImportJobCreatePayload {
            job: Job::from(job),
        })
    }
}

impl From<CalaOutboxImportJobCreateInput> for CalaOutboxImportConfig {
    fn from(input: CalaOutboxImportJobCreateInput) -> Self {
        CalaOutboxImportConfig {
            endpoint: input.endpoint,
        }
    }
}
