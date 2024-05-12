use async_trait::async_trait;
use uuid::Uuid;

use super::current::CurrentJob;

#[async_trait]
pub trait JobRunner: Send + Sync + 'static {
    async fn run(&self, current_job: CurrentJob) -> Result<(), Box<dyn std::error::Error>>;
}

#[async_trait]
pub trait JobInitializer: Send + Sync + 'static {
    async fn init(&self, id: Uuid) -> Result<Box<dyn JobRunner>, Box<dyn std::error::Error>>;
}
