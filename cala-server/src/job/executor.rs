use sqlx::{postgres::types::PgInterval, PgPool, Postgres, Transaction};
use tokio::sync::RwLock;
use tracing::instrument;

use std::{collections::HashMap, sync::Arc};

pub use super::{
    config::*, current::*, entity::*, error::JobError, registry::*, repo::*, traits::*,
};
use crate::primitives::JobId;

#[derive(Clone)]
pub struct JobExecutor {
    config: JobExecutorConfig,
    pool: PgPool,
    registry: Arc<JobRegistry>,
    poller_handle: Option<Arc<tokio::task::JoinHandle<()>>>,
    running_jobs: Arc<RwLock<HashMap<JobId, JobHandle>>>,
    jobs: Jobs,
}

impl JobExecutor {
    pub fn new(
        pool: &PgPool,
        config: JobExecutorConfig,
        registry: JobRegistry,
        jobs: &Jobs,
    ) -> Self {
        Self {
            pool: pool.clone(),
            poller_handle: None,
            config,
            registry: Arc::new(registry),
            running_jobs: Arc::new(RwLock::new(HashMap::new())),
            jobs: jobs.clone(),
        }
    }

    pub async fn spawn_job<I: JobInitializer>(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        job: &Job,
    ) -> Result<(), JobError> {
        if job.job_type != I::job_type() {
            return Err(JobError::JobTypeMismatch(
                job.job_type.clone(),
                I::job_type(),
            ));
        }
        if !self.registry.initializer_exists(&job.job_type) {
            return Err(JobError::NoInitializerPresent);
        }
        sqlx::query!(
            r#"
          INSERT INTO job_executions (id)
          VALUES ($1)
        "#,
            job.id as JobId,
        )
        .execute(&mut **tx)
        .await?;
        Ok(())
    }

    pub async fn start_poll(&mut self) -> Result<(), JobError> {
        let pool = self.pool.clone();
        let server_id = self.config.server_id.clone();
        let poll_interval = self.config.poll_interval;
        let pg_interval = PgInterval::try_from(poll_interval * 4)
            .map_err(|e| JobError::InvalidPollInterval(e.to_string()))?;
        let running_jobs = Arc::clone(&self.running_jobs);
        let registry = Arc::clone(&self.registry);
        let jobs = self.jobs.clone();
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
                    &jobs,
                )
                .await;
                tokio::time::sleep(poll_interval).await;
            }
        });
        self.poller_handle = Some(Arc::new(handle));
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
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
        running_jobs: &Arc<RwLock<HashMap<JobId, JobHandle>>>,
        jobs: &Jobs,
    ) -> Result<(), JobError> {
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
                    &ids as &[JobId],
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
              RETURNING je.id AS "id!: JobId", je.state_json
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
                let job = jobs.find_by_id(row.id).await?;
                let _ = Self::start_job(pool, registry, running_jobs, job, row.state_json).await;
            }
        }
        Ok(())
    }

    #[instrument(
        name = "job_executor.start_job",
        skip(registry, running_jobs, job),
        err
    )]
    async fn start_job(
        pool: &PgPool,
        registry: &Arc<JobRegistry>,
        running_jobs: &Arc<RwLock<HashMap<JobId, JobHandle>>>,
        job: Job,
        job_state: Option<serde_json::Value>,
    ) -> Result<(), JobError> {
        let id = job.id;
        let runner = registry.init_job(&job)?;
        let all_jobs = Arc::clone(running_jobs);
        let pool = pool.clone();
        let handle = tokio::spawn(async move {
            let current_job = CurrentJob::new(id, pool, job_state);
            let _ = runner.run(current_job).await;
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
