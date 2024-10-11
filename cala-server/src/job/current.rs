use serde::{de::DeserializeOwned, Serialize};
use sqlx::{PgPool, Postgres, Transaction};

use super::error::JobError;
use crate::{integration::*, primitives::JobId};

pub struct CurrentJob {
    id: JobId,
    attempt: u32,
    pool: PgPool,
    state_json: Option<serde_json::Value>,
}

impl CurrentJob {
    pub(super) fn new(
        id: JobId,
        attempt: u32,
        pool: PgPool,
        state: Option<serde_json::Value>,
    ) -> Self {
        Self {
            id,
            attempt,
            pool,
            state_json: state,
        }
    }

    pub fn attempt(&self) -> u32 {
        self.attempt
    }

    pub fn state<T: DeserializeOwned>(&self) -> Result<Option<T>, serde_json::Error> {
        if let Some(state) = self.state_json.as_ref() {
            serde_json::from_value(state.clone()).map(Some)
        } else {
            Ok(None)
        }
    }

    pub async fn update_state<T: Serialize>(
        &mut self,
        db: &mut Transaction<'_, Postgres>,
        state: T,
    ) -> Result<(), JobError> {
        let state_json = serde_json::to_value(state).map_err(JobError::CouldNotSerializeState)?;
        sqlx::query!(
            r#"
          UPDATE jobs
          SET state_json = $1
          WHERE id = $2
        "#,
            state_json,
            self.id as JobId
        )
        .execute(&mut **db)
        .await?;
        self.state_json = Some(state_json);
        Ok(())
    }

    pub fn id(&self) -> JobId {
        self.id
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    pub async fn integration(&self, id: IntegrationId) -> Result<Integration, sqlx::Error> {
        Integrations::new(self.pool()).find_by_id(id).await
    }
}
