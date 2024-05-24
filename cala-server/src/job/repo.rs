use sqlx::{PgPool, Postgres, Transaction};

use super::{cursor::*, entity::*, error::*};
use crate::primitives::JobId;
use cala_ledger::{entity::*, query::*};

#[derive(Debug, Clone)]
pub struct Jobs {
    pool: PgPool,
}

impl Jobs {
    pub fn new(pool: &PgPool) -> Self {
        Self { pool: pool.clone() }
    }

    pub async fn create_in_tx(
        &self,
        db: &mut Transaction<'_, Postgres>,
        new_job: NewJob,
    ) -> Result<Job, JobError> {
        let id = new_job.id;
        sqlx::query!(
            r#"INSERT INTO jobs (id, name)
            VALUES ($1, $2)"#,
            id as JobId,
            new_job.name,
        )
        .execute(&mut **db)
        .await?;
        let mut events = new_job.initial_events();
        events.persist(db).await?;
        let job = Job::try_from(events)?;
        Ok(job)
    }

    pub async fn list(
        &self,
        query: PaginatedQueryArgs<JobByNameCursor>,
    ) -> Result<PaginatedQueryRet<Job, JobByNameCursor>, JobError> {
        let rows = sqlx::query_as!(
            GenericEvent,
            r#"
            WITH limited_jobs AS (
              SELECT id, name, created_at
              FROM jobs
              WHERE ((name, id) > ($2, $1)) OR ($1 IS NULL AND $2 IS NULL)
              ORDER BY name, id
              LIMIT $3
            )
            SELECT j.id, e.sequence, e.event,
                j.created_at AS entity_created_at, e.recorded_at AS event_recorded_at
            FROM limited_jobs j
            JOIN job_events e ON j.id = e.id
            ORDER BY j.name, j.id, e.sequence"#,
            query.after.as_ref().map(|c| c.id) as Option<JobId>,
            query.after.map(|c| c.name),
            query.first as i64 + 1
        )
        .fetch_all(&self.pool)
        .await?;

        let (entities, has_next_page) = EntityEvents::load_n::<Job>(rows, query.first)?;
        let mut end_cursor = None;
        if let Some(last) = entities.last() {
            end_cursor = Some(JobByNameCursor {
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
    pub async fn find_by_id(&self, id: JobId) -> Result<Job, JobError> {
        let rows = sqlx::query_as!(
            GenericEvent,
            r#"SELECT a.id, e.sequence, e.event,
                      a.created_at AS entity_created_at, e.recorded_at AS event_recorded_at
            FROM jobs a
            JOIN job_events e ON a.id = e.id
            WHERE a.id = $1
            ORDER BY e.sequence"#,
            id as JobId
        )
        .fetch_all(&self.pool)
        .await?;

        let res = EntityEvents::load_first::<Job>(rows)?;
        Ok(res)
    }
}
