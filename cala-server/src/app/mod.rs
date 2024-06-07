mod config;
mod error;

use sqlx::PgPool;
use tracing::instrument;

use cala_ledger::{query::*, AtomicOperation, CalaLedger};

use crate::{integration::*, job::*};
pub use config::*;
pub use error::*;

#[derive(Clone)]
pub struct CalaApp {
    pool: PgPool,
    ledger: CalaLedger,
    jobs: Jobs,
    job_executor: JobExecutor,
}

impl CalaApp {
    pub(crate) async fn run(
        pool: PgPool,
        config: AppConfig,
        ledger: CalaLedger,
        registry: JobRegistry,
    ) -> Result<Self, ApplicationError> {
        let jobs = Jobs::new(&pool);
        let mut job_executor =
            JobExecutor::new(&pool, config.job_execution.clone(), registry, &jobs);
        job_executor.start_poll().await?;
        Ok(Self {
            pool,
            ledger,
            job_executor,
            jobs,
        })
    }

    pub fn integrations(&self) -> Integrations {
        Integrations::new(&self.pool)
    }

    pub fn ledger(&self) -> &CalaLedger {
        &self.ledger
    }

    #[instrument(name = "cala_server.create_and_spawn_job", skip(self, op, config))]
    pub async fn create_and_spawn_job_in_op<I: JobInitializer + Default, C: serde::Serialize>(
        &self,
        op: &mut AtomicOperation<'_>,
        name: String,
        description: Option<String>,
        config: C,
    ) -> Result<Job, ApplicationError> {
        let new_job = NewJob::builder()
            .name(name)
            .description(description)
            .config(config)?
            .job_type(<I as JobInitializer>::job_type())
            .build()
            .expect("Could not build job");
        let job = self.jobs.create_in_tx(op.tx(), new_job).await?;
        self.job_executor.spawn_job::<I>(op.tx(), &job).await?;
        Ok(job)
    }

    #[instrument(name = "cala_server.list_jobs", skip(self))]
    pub(crate) async fn list_jobs(
        &self,
        query: PaginatedQueryArgs<JobByNameCursor>,
    ) -> Result<PaginatedQueryRet<Job, JobByNameCursor>, ApplicationError> {
        Ok(self.jobs.list(query).await?)
    }
}
