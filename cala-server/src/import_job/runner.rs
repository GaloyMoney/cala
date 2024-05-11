use async_trait::async_trait;
use uuid::Uuid;

use cala_ledger::CalaLedger;

use crate::{
    job::{JobInitializer, JobRunner},
    primitives::ImportJobId,
};

use super::ImportJobs;

pub struct ImportJobInitializer {
    repo: ImportJobs,
    ledger: CalaLedger,
}

impl ImportJobInitializer {
    pub fn new(repo: ImportJobs, ledger: CalaLedger) -> Self {
        Self { repo, ledger }
    }
}

#[async_trait]
impl JobInitializer for ImportJobInitializer {
    async fn init(&self, id: Uuid) -> Result<Box<dyn JobRunner>, Box<dyn std::error::Error>> {
        Ok(self
            .repo
            .find_by_id(ImportJobId::from(id))
            .await?
            .runner(&self.ledger))
    }
}
