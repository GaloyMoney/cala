use async_graphql::*;

use super::job::*;
use crate::{
    app::CalaApp,
    graphql::{primitives::UUID, DbOp, Job},
};

#[derive(InputObject)]
pub struct CalaOutboxImportJobCreateInput {
    pub job_id: UUID,
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
            .jobs()
            .create_and_spawn_in_op::<CalaOutboxImportJobInitializer, _>(
                &mut op,
                input.job_id,
                input.name.clone(),
                input.description.clone(),
                CalaOutboxImportJobState::from(input),
            )
            .await?;
        Ok(CalaOutboxImportJobCreatePayload {
            job: Job::from(job),
        })
    }
}

impl From<CalaOutboxImportJobCreateInput> for CalaOutboxImportJobState {
    fn from(input: CalaOutboxImportJobCreateInput) -> Self {
        Self {
            endpoint: input.endpoint,
            last_synced: Default::default(),
        }
    }
}
