use sqlx::{PgPool, Postgres, Transaction};

use super::{cursor::*, entity::*, error::*};
use crate::primitives::ImportJobId;
use cala_ledger::{entity::*, query::*};

#[derive(Debug, Clone)]
pub struct ImportJobs {
    pool: PgPool,
}

impl ImportJobs {
    pub fn new(pool: &PgPool) -> Self {
        Self { pool: pool.clone() }
    }

    pub async fn create_in_tx(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        new_import_job: NewImportJob,
    ) -> Result<ImportJob, ImportJobError> {
        let id = new_import_job.id;
        sqlx::query!(
            r#"INSERT INTO import_jobs (id, name)
            VALUES ($1, $2)"#,
            id as ImportJobId,
            new_import_job.name,
        )
        .execute(&mut **tx)
        .await?;
        let mut events = new_import_job.initial_events();
        events.persist(tx).await?;
        let import_job = ImportJob::try_from(events)?;
        Ok(import_job)
    }

    pub async fn list(
        &self,
        query: PaginatedQueryArgs<ImportJobByNameCursor>,
    ) -> Result<PaginatedQueryRet<ImportJob, ImportJobByNameCursor>, ImportJobError> {
        let rows = sqlx::query_as!(
            GenericEvent,
            r#"
            WITH jobs AS (
              SELECT id, name, created_at
              FROM import_jobs
              WHERE ((name, id) > ($2, $1)) OR ($1 IS NULL AND $2 IS NULL)
              ORDER BY name, id
              LIMIT $3
            )
            SELECT j.id, e.sequence, e.event,
                j.created_at AS entity_created_at, e.recorded_at AS event_recorded_at
            FROM jobs j
            JOIN import_job_events e ON j.id = e.id
            ORDER BY j.name, j.id, e.sequence"#,
            query.after.as_ref().map(|c| c.id) as Option<ImportJobId>,
            query.after.map(|c| c.name),
            query.first as i64 + 1
        )
        .fetch_all(&self.pool)
        .await?;

        let (entities, has_next_page) = EntityEvents::load_n::<ImportJob>(rows, query.first)?;
        let mut end_cursor = None;
        if let Some(last) = entities.last() {
            end_cursor = Some(ImportJobByNameCursor {
                id: last.id,
                name: last.name.clone(),
            });
        }
        Ok(PaginatedQueryRet {
            entities,
            has_next_page,
            end_cursor,
        })
    }
}
