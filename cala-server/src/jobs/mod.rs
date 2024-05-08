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

pub struct JobExecutor {
    config: JobExecutorConfig,
    pool: PgPool,
    registry: JobRegistry,
    poller_handle: Option<Arc<tokio::task::JoinHandle<()>>>,
    running_jobs: Arc<RwLock<HashMap<Uuid, JobHandle>>>,
}

impl JobExecutor {
    pub fn new(pool: &PgPool, config: JobExecutorConfig) -> Self {
        unimplemented!()
        // Self {
        //     pool: pool.clone(),
        //     poller_handle: None,
        //     config,
        //     running_jobs: Arc::new(RwLock::new(HashMap::new())),
        // }
    }

    pub async fn spawn_job(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        id: Uuid,
        job_type: JobType,
    ) -> Result<(), JobExecutorError> {
        sqlx::query!(
            r#"
          INSERT INTO job_executions (id, type)
          VALUES ($1, $2)
        "#,
            id,
            job_type as JobType
        )
        .execute(&mut **tx)
        .await?;
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
