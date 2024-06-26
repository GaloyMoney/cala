use serde::{de::DeserializeOwned, Serialize};
use sqlx::{PgPool, Postgres, Transaction};

use super::{entity::*, error::JobError, repo::*};
use crate::{integration::*, primitives::JobId};

pub struct CurrentJob {
    id: JobId,
    pool: PgPool,
    state_json: Option<serde_json::Value>,
}

impl CurrentJob {
    pub(super) fn new(id: JobId, pool: PgPool, state: Option<serde_json::Value>) -> Self {
        Self {
            id,
            pool,
            state_json: state,
        }
    }

    pub fn state<T: DeserializeOwned>(&self) -> Result<Option<T>, serde_json::Error> {
        if let Some(state) = self.state_json.as_ref() {
            serde_json::from_value(state.clone()).map(Some)
        } else {
            Ok(None)
        }
    }

    pub async fn update_execution_state<T: Serialize>(
        &mut self,
        db: &mut Transaction<'_, Postgres>,
        state: T,
    ) -> Result<(), JobError> {
        let state_json = serde_json::to_value(state).map_err(JobError::CouldNotSerializeState)?;
        sqlx::query!(
            r#"
          UPDATE job_executions
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

    pub async fn persist_in_tx(
        &self,
        db: &mut Transaction<'_, Postgres>,
        entity: &mut Job,
    ) -> Result<(), JobError> {
        JobRepo::new(self.pool()).persist_in_tx(db, entity).await
    }

    pub async fn integration(&self, id: IntegrationId) -> Result<Integration, sqlx::Error> {
        Integrations::new(self.pool()).find_by_id(id).await
    }
}
