mod config;
mod error;

use sqlx::PgPool;
use tracing::instrument;

use cala_ledger::{query::*, CalaLedger};

use crate::job::*;
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
    ) -> Result<Self, ApplicationError> {
        let jobs = Jobs::new(&pool);
        let registry = JobRegistry::new();
        // registry.add_initializer(
        //     CALA_OUTBOX_IMPORT_JOB_TYPE,
        //     Box::new(ImportJobInitializer::new(
        //         import_jobs.clone(),
        //         ledger.clone(),
        //     )),
        // );
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

    pub(crate) fn ledger(&self) -> &CalaLedger {
        &self.ledger
    }

    #[instrument(name = "cala_server.create_and_spawn_job", skip(self, config))]
    pub async fn create_and_spawn_job<C: serde::Serialize>(
        &self,
        name: String,
        description: Option<String>,
        job_type: JobType,
        config: C,
    ) -> Result<Job, ApplicationError> {
        let new_job = NewJob::builder()
            .name(name)
            .description(description)
            .config(config)?
            .job_type(job_type)
            .build()
            .expect("Could not build job");
        let mut tx = self.pool.begin().await?;
        let job = self.jobs.create_in_tx(&mut tx, new_job).await?;
        self.job_executor.spawn_job(&mut tx, &job).await?;
        tx.commit().await?;
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
