mod config;
pub mod error;

use sqlx::{postgres::types::PgInterval, PgPool, Postgres, Transaction};
use tokio::sync::RwLock;
use tracing::instrument;
use uuid::Uuid;

use std::{collections::HashMap, sync::Arc};

use crate::{
    import_job::{error::ImportJobError, *},
    primitives::*,
};
pub use config::*;
use error::JobExecutionError;

struct JobHandle(Option<tokio::task::JoinHandle<()>>);
impl Drop for JobHandle {
    fn drop(&mut self) {
        if let Some(handle) = self.0.take() {
            handle.abort();
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, sqlx::Type)]
#[sqlx(type_name = "JobType", rename_all = "snake_case")]
pub enum JobType {
    Import,
}

#[derive(Clone)]
pub struct JobExecution {
    pool: PgPool,
    import_jobs: ImportJobs,
    import_job_runner_deps: ImportJobRunnerDeps,
    config: JobExecutionConfig,
    poller_handle: Option<Arc<tokio::task::JoinHandle<()>>>,
    running_jobs: Arc<RwLock<HashMap<Uuid, JobHandle>>>,
}

impl JobExecution {
    pub fn new(
        pool: &PgPool,
        config: JobExecutionConfig,
        import_jobs: ImportJobs,
        import_job_runner_deps: ImportJobRunnerDeps,
    ) -> Self {
        Self {
            pool: pool.clone(),
            poller_handle: None,
            config,
            import_jobs,
            import_job_runner_deps,
            running_jobs: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn start_poll(&mut self) -> Result<(), JobExecutionError> {
        let pool = self.pool.clone();
        let server_id = self.config.server_id.clone();
        let poll_interval = self.config.poll_interval;
        let pg_interval = PgInterval::try_from(poll_interval * 4)
            .map_err(|e| JobExecutionError::InvalidPollInterval(e.to_string()))?;
        let running_jobs = Arc::clone(&self.running_jobs);
        let import_jobs = self.import_jobs.clone();
        let import_job_runner_deps = self.import_job_runner_deps.clone();
        let handle = tokio::spawn(async move {
            let poll_limit = 2;
            let mut keep_alive = false;
            loop {
                let _ = Self::poll_jobs(
                    &pool,
                    &mut keep_alive,
                    server_id.clone(),
                    poll_limit,
                    pg_interval.clone(),
                    &running_jobs,
                    &import_jobs,
                    &import_job_runner_deps,
                )
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

    #[allow(clippy::too_many_arguments)]
    #[instrument(
        name = "job_execution.poll_jobs",
        skip(pool, running_jobs, import_jobs, import_job_runner_deps,),
        fields(n_jobs_to_spawn, n_jobs_running),
        err
    )]
    async fn poll_jobs(
        pool: &PgPool,
        keep_alive: &mut bool,
        server_id: String,
        poll_limit: u32,
        pg_interval: PgInterval,
        running_jobs: &Arc<RwLock<HashMap<Uuid, JobHandle>>>,
        import_jobs: &ImportJobs,
        import_job_runner_deps: &ImportJobRunnerDeps,
    ) -> Result<(), JobExecutionError> {
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
              RETURNING je.id, je.type AS "job_type: JobType"
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
                let id = row.id;
                let job_type = row.job_type;
                let _ = Self::spawn_job(
                    running_jobs,
                    id,
                    job_type,
                    import_job_runner_deps,
                    import_jobs,
                )
                .await;
            }
        }
        Ok(())
    }

    #[instrument(
        name = "job_execution.spawn_job",
        skip(running_jobs, deps, import_jobs)
    )]
    async fn spawn_job(
        running_jobs: &Arc<RwLock<HashMap<Uuid, JobHandle>>>,
        id: Uuid,
        _job_type: JobType,
        deps: &ImportJobRunnerDeps,
        import_jobs: &ImportJobs,
    ) -> Result<(), ImportJobError> {
        let job = import_jobs.find_by_id(ImportJobId::from(id)).await?;
        let runner = job.runner(deps);
        let all_jobs = Arc::clone(running_jobs);
        let handle = tokio::spawn(async move {
            let _ = runner.run().await;
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            all_jobs.write().await.remove(&id);
        });
        running_jobs
            .write()
            .await
            .insert(id, JobHandle(Some(handle)));
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
