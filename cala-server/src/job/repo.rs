use sqlx::{PgPool, Postgres, Transaction};

use crate::primitives::JobId;

use super::{entity::*, error::*};

#[derive(Debug, Clone)]
pub(super) struct JobRepo {
    pool: PgPool,
}

impl JobRepo {
    pub fn new(pool: &PgPool) -> Self {
        Self { pool: pool.clone() }
    }

    pub async fn create_in_tx(
        &self,
        db: &mut Transaction<'_, Postgres>,
        job: Job,
    ) -> Result<Job, JobError> {
        sqlx::query!(
            r#"INSERT INTO jobs (id, type, name, description, state_json)
            VALUES ($1, $2, $3, $4, $5)"#,
            job.id as JobId,
            &job.job_type as &JobType,
            &job.name,
            job.description.as_ref(),
            job.state::<serde_json::Value>()
                .expect("Could not serialize state")
        )
        .execute(&mut **db)
        .await?;
        Ok(job)
    }

    pub async fn persist_in_tx(
        &self,
        db: &mut Transaction<'_, Postgres>,
        job: Job,
    ) -> Result<(), JobError> {
        sqlx::query!(
            r#"UPDATE jobs
               SET completed_at = $2,
                   last_error = $3,
                   modified_at = NOW()
               WHERE id = $1"#,
            job.id as JobId,
            job.completed_at,
            job.last_error,
        )
        .execute(&mut **db)
        .await?;
        Ok(())
    }

    pub async fn find_by_id(&self, id: JobId) -> Result<Job, JobError> {
        let row = sqlx::query!(
            r#"SELECT id as "id: JobId", type AS job_type, name, description, state_json, completed_at, last_error
            FROM jobs
            WHERE id = $1"#,
            id as JobId
        )
        .fetch_one(&self.pool)
        .await?;
        let mut job = Job::new(
            row.name,
            JobType::from_string(row.job_type),
            row.description,
            row.state_json,
        );
        job.id = row.id;
        job.completed_at = row.completed_at;
        job.last_error = row.last_error;
        Ok(job)
    }
}
