use async_graphql::*;

use super::primitives::*;

#[derive(InputObject)]
pub struct ImportJobCreateInput {
    pub name: String,
    pub description: Option<String>,
    pub endpoint: String,
}

#[derive(SimpleObject)]
pub struct ImportJob {
    pub id: ID,
    pub import_job_id: UUID,
    pub name: String,
    pub description: Option<String>,
}

#[derive(SimpleObject)]
pub struct ImportJobCreatePayload {
    pub journal: ImportJob,
}
