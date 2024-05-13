use async_trait::async_trait;

use super::{current::CurrentJob, entity::Job};

#[async_trait]
pub trait JobInitializer: Send + Sync + 'static {
    async fn init(&self, job: &Job) -> Result<Box<dyn JobRunner>, Box<dyn std::error::Error>>;
}

#[async_trait]
pub trait JobRunner: Send + Sync + 'static {
    async fn run(&self, current_job: CurrentJob) -> Result<(), Box<dyn std::error::Error>>;
}
