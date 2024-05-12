use async_graphql::*;

use super::config::*;
use crate::{app::CalaApp, graphql::Job, job::JobType};

#[derive(InputObject)]
pub struct CalaOutboxImportJobCreateInput {
    pub job_name: String,
    pub description: Option<String>,
    pub endpoint: String,
}

#[derive(SimpleObject)]
pub struct CalaOutboxImportJobCreatePayload {
    pub job: Job,
}

#[derive(Default)]
pub struct Mutation;

#[async_graphql::Object(name = "BigQueryMutation")]
impl Mutation {
    async fn cala_outbox_import_job_create(
        &self,
        ctx: &Context<'_>,
        input: CalaOutboxImportJobCreateInput,
    ) -> async_graphql::Result<CalaOutboxImportJobCreatePayload> {
        let app = ctx.data_unchecked::<CalaApp>();
        let name = input.job_name.clone();
        let job = app
            .create_and_spawn_job(
                name,
                None,
                super::job::CALA_OUTBOX_IMPORT_JOB_TYPE,
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
