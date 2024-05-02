use sqlx::PgPool;

use super::{entity::*, error::*};
use crate::primitives::ImportJobId;

#[derive(Debug, Clone)]
pub struct ImportJobs {
    pool: PgPool,
}

impl ImportJobs {
    pub fn new(pool: &PgPool) -> Self {
        Self { pool: pool.clone() }
    }

    pub async fn create(&self, new_import_job: NewImportJob) -> Result<ImportJob, ImportJobError> {
        let mut tx = self.pool.begin().await?;
        let id = new_import_job.id;
        sqlx::query!(
            r#"INSERT INTO import_jobs (id, name)
            VALUES ($1, $2)"#,
            id as ImportJobId,
            new_import_job.name,
        )
        .execute(&mut *tx)
        .await?;
        let mut events = new_import_job.initial_events();
        events.persist(&mut tx).await?;
        let import_job = ImportJob::try_from(events)?;
        tx.commit().await?;
        Ok(import_job)
    }
}
