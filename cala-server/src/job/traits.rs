use async_trait::async_trait;

use super::{current::CurrentJob, entity::Job};
use cala_ledger::CalaLedger;

#[async_trait]
pub trait JobInitializer: Send + Sync + 'static {
    async fn init(
        &self,
        job: &Job,
        ledger: &CalaLedger,
    ) -> Result<Box<dyn JobRunner>, Box<dyn std::error::Error>>;
}

#[async_trait]
pub trait JobRunner: Send + Sync + 'static {
    async fn run(&self, current_job: CurrentJob) -> Result<(), Box<dyn std::error::Error>>;
}
