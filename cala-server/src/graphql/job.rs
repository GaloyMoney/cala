use async_graphql::*;

use super::{convert::ToGlobalId, primitives::*};

#[derive(SimpleObject)]
pub struct Job {
    pub id: ID,
    pub job_id: UUID,
}

impl ToGlobalId for job::JobId {
    fn to_global_id(&self) -> async_graphql::types::ID {
        async_graphql::types::ID::from(format!("job:{self}"))
    }
}

impl From<job::Job> for Job {
    fn from(job: job::Job) -> Self {
        Self {
            id: job.id.to_global_id(),
            job_id: UUID::from(job.id),
        }
    }
}
