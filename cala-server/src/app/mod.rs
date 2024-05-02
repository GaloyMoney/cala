mod error;

use sqlx::{Pool, Postgres};
use tracing::instrument;

use cala_ledger::{query::*, CalaLedger};

use crate::import_job::*;
pub use error::*;

#[derive(Clone)]
pub struct CalaApp {
    _pool: Pool<Postgres>,
    ledger: CalaLedger,
    import_jobs: ImportJobs,
}

impl CalaApp {
    pub fn new(pool: Pool<Postgres>, ledger: CalaLedger) -> Self {
        let import_jobs = ImportJobs::new(&pool);
        Self {
            _pool: pool,
            ledger,
            import_jobs,
        }
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
            .import_config(ImportJobConfig::CalaOutbox(CalaOutboxImportConfig {
                endpoint,
            }))
            .build()
            .expect("Could not build import job");
        Ok(self.import_jobs.create(new_import_job).await?)
    }

    #[instrument(name = "cala_server.list_import_jobs", skip(self))]
    pub async fn list_import_jobs(
        &self,
        query: PaginatedQueryArgs<ImportJobByNameCursor>,
    ) -> Result<PaginatedQueryRet<ImportJob, ImportJobByNameCursor>, ApplicationError> {
        Ok(self.import_jobs.list(query).await?)
    }
}
