mod error;

use sqlx::{Pool, Postgres};

use cala_ledger::CalaLedger;

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
}
