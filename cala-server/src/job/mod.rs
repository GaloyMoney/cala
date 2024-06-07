mod config;
mod current;
mod cursor;
mod entity;
mod executor;
mod registry;
mod repo;
mod traits;

pub mod error;

use cala_ledger::{query::*, AtomicOperation};
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

    #[instrument(name = "cala_server.jobs.create_and_spawn", skip(self, op, config))]
    pub async fn create_and_spawn_in_op<I: JobInitializer + Default, C: serde::Serialize>(
        &self,
        op: &mut AtomicOperation<'_>,
        name: String,
        description: Option<String>,
        config: C,
    ) -> Result<Job, JobError> {
        let new_job = NewJob::builder()
            .name(name)
            .description(description)
            .config(config)?
            .job_type(<I as JobInitializer>::job_type())
            .build()
            .expect("Could not build job");
        let job = self.repo.create_in_tx(op.tx(), new_job).await?;
        self.executor.spawn_job::<I>(op.tx(), &job).await?;
        Ok(job)
    }

    pub async fn list(
        &self,
        query: PaginatedQueryArgs<JobByNameCursor>,
    ) -> Result<PaginatedQueryRet<Job, JobByNameCursor>, JobError> {
        self.repo.list(query).await
    }

    pub(crate) async fn start_poll(&mut self) -> Result<(), JobError> {
        self.executor.start_poll().await
    }
}
