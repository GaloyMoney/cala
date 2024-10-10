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

    fn retry_on_error_settings() -> RetrySettings
    where
        Self: Sized,
    {
        Default::default()
    }

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

pub struct RetrySettings {
    pub n_attempts: u32,
    pub min_backoff: std::time::Duration,
    pub max_backoff: std::time::Duration,
}

impl RetrySettings {
    pub(super) fn next_attempt_at(&self, attempt: u32) -> DateTime<Utc> {
        let backoff = std::cmp::min(
            self.min_backoff.as_secs() * 2u64.pow(attempt - 1),
            self.max_backoff.as_secs(),
        );
        chrono::Utc::now() + std::time::Duration::from_secs(backoff)
    }
}

impl Default for RetrySettings {
    fn default() -> Self {
        Self {
            n_attempts: 5,
            min_backoff: std::time::Duration::from_secs(1),
            max_backoff: std::time::Duration::from_secs(60),
        }
    }
}
