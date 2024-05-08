mod config;
pub mod error;
mod registry;
mod traits;

use sqlx::{postgres::types::PgInterval, PgPool, Postgres, Transaction};
use tokio::sync::RwLock;
use tracing::instrument;
use uuid::Uuid;

use std::{collections::HashMap, sync::Arc};

pub use config::*;
use error::JobExecutorError;
pub use registry::*;
pub use traits::*;

#[derive(Clone)]
pub struct JobExecutor {
    config: JobExecutorConfig,
    pool: PgPool,
    registry: Arc<JobRegistry>,
    poller_handle: Option<Arc<tokio::task::JoinHandle<()>>>,
    running_jobs: Arc<RwLock<HashMap<Uuid, JobHandle>>>,
}

impl JobExecutor {
    pub fn new(pool: &PgPool, config: JobExecutorConfig, registry: JobRegistry) -> Self {
        Self {
            pool: pool.clone(),
            poller_handle: None,
            config,
            registry: Arc::new(registry),
            running_jobs: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn spawn_job<T: Into<JobTemplate>>(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        job: T,
    ) -> Result<(), JobExecutorError> {
        let template: JobTemplate = job.into();
        if !self.registry.initializer_exists(template.job_type) {
            return Err(JobExecutorError::InvalidJobType(template.job_type));
        }

        sqlx::query!(
            r#"
          INSERT INTO job_executions (id, job_type)
          VALUES ($1, $2)
        "#,
            template.id,
            template.job_type
        )
        .execute(&mut **tx)
        .await?;
        Ok(())
    }

    pub async fn start_poll(&mut self) -> Result<(), JobExecutorError> {
        let pool = self.pool.clone();
        let server_id = self.config.server_id.clone();
        let poll_interval = self.config.poll_interval;
        let pg_interval = PgInterval::try_from(poll_interval * 4)
            .map_err(|e| JobExecutorError::InvalidPollInterval(e.to_string()))?;
        let running_jobs = Arc::clone(&self.running_jobs);
        let registry = Arc::clone(&self.registry);
        let handle = tokio::spawn(async move {
            let poll_limit = 2;
            let mut keep_alive = false;
            loop {
                let _ = Self::poll_jobs(
                    &pool,
                    &registry,
                    &mut keep_alive,
                    &server_id,
                    poll_limit,
                    pg_interval.clone(),
                    &running_jobs,
                )
                .await;
                tokio::time::sleep(poll_interval).await;
            }
        });
        self.poller_handle = Some(Arc::new(handle));
        Ok(())
    }

    #[instrument(
        name = "job_executor.poll_jobs",
        skip(pool, registry, running_jobs),
        fields(n_jobs_to_spawn, n_jobs_running),
        err
    )]
    async fn poll_jobs(
        pool: &PgPool,
        registry: &Arc<JobRegistry>,
        keep_alive: &mut bool,
        server_id: &str,
        poll_limit: u32,
        pg_interval: PgInterval,
        running_jobs: &Arc<RwLock<HashMap<Uuid, JobHandle>>>,
    ) -> Result<(), JobExecutorError> {
        let span = tracing::Span::current();
        span.record("keep_alive", *keep_alive);
        {
            let jobs = running_jobs.read().await;
            span.record("n_jobs_running", jobs.len());
            if *keep_alive {
                let ids = jobs.keys().cloned().collect::<Vec<_>>();
                sqlx::query!(
                    r#"
                    UPDATE job_executions
                    SET reschedule_after = NOW() + $3::interval,
                        executing_server_id = $2
                    WHERE id = ANY($1)
                    "#,
                    &ids,
                    server_id,
                    pg_interval
                )
                .fetch_all(pool)
                .await?;
            }
        }
        *keep_alive = !*keep_alive;
        let rows = sqlx::query!(
            r#"
              WITH selected_jobs AS (
                  SELECT id
                  FROM job_executions
                  WHERE reschedule_after < NOW()
                  LIMIT $2
                  FOR UPDATE
              )
              UPDATE job_executions AS je
              SET reschedule_after = NOW() + $3::interval,
                  executing_server_id = $1
              FROM selected_jobs
              WHERE je.id = selected_jobs.id
              RETURNING je.id, je.job_type
              "#,
            server_id,
            poll_limit as i32,
            pg_interval
        )
        .fetch_all(pool)
        .await?;
        span.record("n_jobs_to_spawn", rows.len());
        if !rows.is_empty() {
            for row in rows {
                let _ = Self::start_job(registry, running_jobs, &row.job_type, row.id).await;
            }
        }
        Ok(())
    }

    #[instrument(name = "job_executor.start_job", skip(registry, running_jobs))]
    async fn start_job(
        registry: &Arc<JobRegistry>,
        running_jobs: &Arc<RwLock<HashMap<Uuid, JobHandle>>>,
        job_type: &str,
        id: Uuid,
    ) -> Result<(), JobExecutorError> {
        let runner = registry.init_job(job_type, id).await?;
        let all_jobs = Arc::clone(running_jobs);
        let handle = tokio::spawn(async move {
            let _ = runner.run().await;
            all_jobs.write().await.remove(&id);
        });
        running_jobs
            .write()
            .await
            .insert(id, JobHandle(Some(handle)));
        Ok(())
    }
}

struct JobHandle(Option<tokio::task::JoinHandle<()>>);
impl Drop for JobHandle {
    fn drop(&mut self) {
        if let Some(handle) = self.0.take() {
            handle.abort();
        }
    }
}
