use async_graphql::*;

use super::job::*;
use crate::{
    app::CalaApp,
    graphql::{
        primitives::{DbOp, UUID},
        Job,
    },
};

#[derive(InputObject)]
pub struct CalaOutboxImportJobCreateInput {
    pub job_id: UUID,
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
            .create_and_spawn_in_op(
                &mut *op,
                input.job_id,
                CalaOutboxImportJobConfig::new(input.endpoint),
            )
            .await?;
        Ok(CalaOutboxImportJobCreatePayload {
            job: Job::from(job),
        })
    }
}
