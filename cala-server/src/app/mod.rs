mod config;
mod error;

use sqlx::PgPool;
use tracing::instrument;

use cala_ledger::{query::*, CalaLedger};

use crate::{import_job::*, jobs::*};
pub use config::*;
pub use error::*;

#[derive(Clone)]
pub struct CalaApp {
    pool: PgPool,
    ledger: CalaLedger,
    import_jobs: ImportJobs,
    // job_execution: JobExecution,
}

impl CalaApp {
    pub async fn run(
        pool: PgPool,
        config: AppConfig,
        ledger: CalaLedger,
    ) -> Result<Self, ApplicationError> {
        let import_jobs = ImportJobs::new(&pool);
        let import_deps = ImportJobRunnerDeps {};
        // let mut job_execution = JobExecutor::new(
        //     &pool,
        //     config.job_execution.clone(),
        //     import_jobs.clone(),
        //     import_deps,
        // );
        // job_execution.start_poll().await?;
        Ok(Self {
            pool,
            ledger,
            import_jobs,
            // job_execution,
        })
    }

    pub fn ledger(&self) -> &CalaLedger {
        &self.ledger
    }

    #[instrument(name = "cala_server.create_import_job", skip(self))]
    pub async fn create_import_job(
        &self,
        name: String,
        description: Option<String>,
        endpoint: String,
    ) -> Result<ImportJob, ApplicationError> {
        let new_import_job = NewImportJob::builder()
            .name(name)
            .description(description)
            .config(ImportJobConfig::CalaOutbox(
                cala_outbox::CalaOutboxImportConfig { endpoint },
            ))
            .build()
            .expect("Could not build import job");
        let mut tx = self.pool.begin().await?;
        let job = self
            .import_jobs
            .create_in_tx(&mut tx, new_import_job)
            .await?;
        // self.job_execution
        //     .register_import_job(&mut tx, &job)
        //     .await?;
        tx.commit().await?;
        Ok(job)
    }

    #[instrument(name = "cala_server.list_import_jobs", skip(self))]
    pub async fn list_import_jobs(
        &self,
        query: PaginatedQueryArgs<ImportJobByNameCursor>,
    ) -> Result<PaginatedQueryRet<ImportJob, ImportJobByNameCursor>, ApplicationError> {
        Ok(self.import_jobs.list(query).await?)
    }
}
