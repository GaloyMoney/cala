use async_trait::async_trait;
use chrono::{DateTime, Utc};

use super::{
    current::CurrentJob,
    entity::{Job, JobType},
};
use cala_ledger::CalaLedger;

pub trait JobInitializer: Send + Sync + 'static {
    fn job_type() -> JobType
    where
        Self: Sized;

    fn init(
        &self,
        job: Job,
        ledger: &CalaLedger,
    ) -> Result<Box<dyn JobRunner>, Box<dyn std::error::Error>>;
}

pub enum JobCompletion {
    Complete,
    CompleteWithTx(sqlx::Transaction<'static, sqlx::Postgres>),
    RescheduleAt(DateTime<Utc>),
    RescheduleAtWithTx(sqlx::Transaction<'static, sqlx::Postgres>, DateTime<Utc>),
}

#[async_trait]
pub trait JobRunner: Send + Sync + 'static {
    async fn run(
        &mut self,
        current_job: CurrentJob,
    ) -> Result<JobCompletion, Box<dyn std::error::Error>>;
}
