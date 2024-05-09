use async_trait::async_trait;
use uuid::Uuid;

use crate::{
    jobs::{JobInitializer, JobRunner},
    primitives::ImportJobId,
};

use super::ImportJobs;

#[derive(Clone)]
pub struct ImportJobRunnerDeps {}

pub struct ImportJobInitializer {
    repo: ImportJobs,
    deps: ImportJobRunnerDeps,
}

impl ImportJobInitializer {
    pub fn new(repo: ImportJobs, deps: ImportJobRunnerDeps) -> Self {
        Self { repo, deps }
    }
}

#[async_trait]
impl JobInitializer for ImportJobInitializer {
    async fn init(&self, id: Uuid) -> Result<Box<dyn JobRunner>, Box<dyn std::error::Error>> {
        Ok(self
            .repo
            .find_by_id(ImportJobId::from(id))
            .await?
            .runner(&self.deps))
    }
}
