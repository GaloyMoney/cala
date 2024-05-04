mod config;
pub mod error;

use sqlx::{PgPool, Postgres, Transaction};

use std::sync::Arc;

use crate::{import_job::ImportJob, primitives::ImportJobId};
pub use config::*;
use error::JobExecutionError;

#[derive(Clone)]
pub struct JobExecution {
    pool: PgPool,
    config: JobExecutionConfig,
    poller_handle: Option<Arc<tokio::task::JoinHandle<()>>>,
}

impl JobExecution {
    pub fn new(pool: &PgPool, config: JobExecutionConfig) -> Self {
        Self {
            pool: pool.clone(),
            poller_handle: None,
            config,
        }
    }

    pub async fn start_poll(&mut self) -> Result<(), JobExecutionError> {
        let pool = self.pool.clone();
        let server_id = self.config.server_id.clone();
        let poll_interval = self.config.poll_interval;
        let handle = tokio::spawn(async move {
            loop {
                let res = sqlx::query!(
                    r#"
                    UPDATE job_executions
                    SET reschedule_after = NOW() + INTERVAL '20 second',
                    executing_server_id = $1
                    WHERE reschedule_after < NOW()
                "#,
                    server_id
                )
                .fetch_all(&pool)
                .await;
                tokio::time::sleep(poll_interval).await;
            }
        });
        self.poller_handle = Some(Arc::new(handle));
        Ok(())
    }

    pub async fn register_import_job(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        job: &ImportJob,
    ) -> Result<(), JobExecutionError> {
        sqlx::query!(
            r#"
          INSERT INTO job_executions (id, type)
          VALUES ($1, 'import')
        "#,
            job.id as ImportJobId,
        )
        .execute(&mut **tx)
        .await?;
        Ok(())
    }
}

impl Drop for JobExecution {
    fn drop(&mut self) {
        if let Some(handle) = self.poller_handle.take() {
            handle.abort();
        }
    }
}
