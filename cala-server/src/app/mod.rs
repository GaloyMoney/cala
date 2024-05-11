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
    job_executor: JobExecutor,
}

impl CalaApp {
    pub(crate) async fn run(
        pool: PgPool,
        config: AppConfig,
        ledger: CalaLedger,
    ) -> Result<Self, ApplicationError> {
        let import_jobs = ImportJobs::new(&pool);
        let mut registry = JobRegistry::new();
        registry.add_initializer(
            CALA_OUTBOX_IMPORT_JOB_TYPE,
            Box::new(ImportJobInitializer::new(
                import_jobs.clone(),
                ledger.clone(),
            )),
        );
        let mut job_executor = JobExecutor::new(&pool, config.job_execution.clone(), registry);
        job_executor.start_poll().await?;
        Ok(Self {
            pool,
            ledger,
            import_jobs,
            job_executor,
        })
    }

    pub(crate) fn ledger(&self) -> &CalaLedger {
        &self.ledger
    }

    #[instrument(name = "cala_server.create_import_job", skip(self))]
    pub(crate) async fn create_import_job(
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
        self.job_executor.spawn_job(&mut tx, &job).await?;
        tx.commit().await?;
        Ok(job)
    }

    #[instrument(name = "cala_server.list_import_jobs", skip(self))]
    pub(crate) async fn list_import_jobs(
        &self,
        query: PaginatedQueryArgs<ImportJobByNameCursor>,
    ) -> Result<PaginatedQueryRet<ImportJob, ImportJobByNameCursor>, ApplicationError> {
        Ok(self.import_jobs.list(query).await?)
    }
}
