mod config;
mod current;
mod cursor;
mod entity;
mod executor;
mod registry;
mod repo;
mod traits;

pub mod error;

use cala_ledger::LedgerOperation;
use chrono::{DateTime, Utc};
use sqlx::PgPool;
use tracing::instrument;

pub use config::*;
pub use current::*;
pub use cursor::*;
pub use entity::*;
pub use registry::*;
pub use traits::*;

use error::*;
use executor::*;
use repo::*;

#[derive(Clone)]
pub struct Jobs {
    _pool: PgPool,
    repo: JobRepo,
    executor: JobExecutor,
}

impl Jobs {
    pub fn new(pool: &PgPool, config: JobExecutorConfig, registry: JobRegistry) -> Self {
        let repo = JobRepo::new(pool);
        let executor = JobExecutor::new(pool, config, registry, &repo);
        Self {
            _pool: pool.clone(),
            repo,
            executor,
        }
    }

    #[instrument(name = "cala_server.jobs.create_and_spawn", skip(self, op, data))]
    pub async fn create_and_spawn_in_op<'a, I: JobInitializer + Default, D: serde::Serialize>(
        &self,
        op: &mut LedgerOperation<'a>,
        id: impl Into<JobId> + std::fmt::Debug,
        name: String,
        description: Option<String>,
        data: D,
    ) -> Result<Job, JobError> {
        let new_job = Job::new(name, <I as JobInitializer>::job_type(), description, data);
        let job = self.repo.create_in_op(op, new_job).await?;
        self.executor.spawn_job::<I>(op, &job, None).await?;
        Ok(job)
    }

    #[instrument(name = "cala_server.jobs.create_and_spawn_at", skip(self, op, data))]
    pub async fn create_and_spawn_at_in_op<I: JobInitializer + Default, D: serde::Serialize>(
        &self,
        op: &mut LedgerOperation<'_>,
        name: String,
        description: Option<String>,
        data: D,
        schedule_at: DateTime<Utc>,
    ) -> Result<Job, JobError> {
        let new_job = Job::new(name, <I as JobInitializer>::job_type(), description, data);
        let job = self.repo.create_in_op(op, new_job).await?;
        self.executor
            .spawn_job::<I>(op.tx(), &job, Some(schedule_at))
            .await?;
        Ok(job)
    }

    #[instrument(name = "cala_server.jobs.find", skip(self))]
    pub async fn find(&self, id: JobId) -> Result<Job, JobError> {
        self.repo.find_by_id(id).await
    }

    pub(crate) async fn start_poll(&mut self) -> Result<(), JobError> {
        self.executor.start_poll().await
    }
}
