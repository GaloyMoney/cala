use cached::proc_macro::cached;
#[cfg(feature = "import")]
use chrono::{DateTime, Utc};
use sqlx::{PgPool, Postgres, Transaction};

use std::sync::Arc;

use crate::entity::*;
#[cfg(feature = "import")]
use crate::primitives::DataSourceId;

use super::{entity::*, error::*};

#[derive(Debug, Clone)]
pub(super) struct TxTemplateRepo {
    pool: PgPool,
}

impl TxTemplateRepo {
    pub fn new(pool: &PgPool) -> Self {
        Self { pool: pool.clone() }
    }

    pub async fn create_in_tx(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        new_tx_template: NewTxTemplate,
    ) -> Result<EntityUpdate<TxTemplate>, TxTemplateError> {
        let id = new_tx_template.id;
        sqlx::query!(
            r#"INSERT INTO cala_tx_templates (id, code)
            VALUES ($1, $2)"#,
            id as TxTemplateId,
            new_tx_template.code,
        )
        .execute(&mut **tx)
        .await?;
        let mut events = new_tx_template.initial_events();
        let n_new_events = events.persist(tx).await?;
        let tx_template = TxTemplate::try_from(events)?;
        Ok(EntityUpdate {
            entity: tx_template,
            n_new_events,
        })
    }

    pub async fn find_latest_version(
        &self,
        code: &str,
    ) -> Result<Arc<TxTemplateValues>, TxTemplateError> {
        let row = sqlx::query!(
            r#"
            SELECT t.id AS "id?: TxTemplateId", MAX(e.sequence) AS "version" 
            FROM cala_tx_templates t
            JOIN cala_tx_template_events e ON t.id = e.id
            WHERE t.code = $1
            GROUP BY t.id"#,
            code,
        )
        .fetch_optional(&self.pool)
        .await?;
        if let Some(row) = row {
            if let (Some(id), Some(version)) = (row.id, row.version) {
                return find_versioned_template_cached(&self.pool, id, version).await;
            }
        }
        Err(TxTemplateError::NotFound)
    }

    #[cfg(feature = "import")]
    pub async fn import(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        recorded_at: DateTime<Utc>,
        origin: DataSourceId,
        tx_template: &mut TxTemplate,
    ) -> Result<(), TxTemplateError> {
        sqlx::query!(
            r#"INSERT INTO cala_tx_templates (data_source_id, id, code, created_at)
            VALUES ($1, $2, $3, $4)"#,
            origin as DataSourceId,
            tx_template.values().id as TxTemplateId,
            tx_template.values().code,
            recorded_at
        )
        .execute(&mut **tx)
        .await?;
        tx_template
            .events
            .persisted_at(tx, origin, recorded_at)
            .await?;
        Ok(())
    }
}

#[cached(
    key = "(TxTemplateId, i32)",
    convert = "{ (id, version) }",
    result = true,
    sync_writes = true
)]
async fn find_versioned_template_cached(
    pool: &PgPool,
    id: TxTemplateId,
    version: i32,
) -> Result<Arc<TxTemplateValues>, TxTemplateError> {
    let row = sqlx::query!(
        r#"
          SELECT event 
          FROM cala_tx_template_events
          WHERE id = $1 AND sequence = $2"#,
        id as TxTemplateId,
        version,
    )
    .fetch_optional(pool)
    .await?;
    if let Some(row) = row {
        let event: TxTemplateEvent = serde_json::from_value(row.event)?;
        Ok(Arc::new(event.into_values()))
    } else {
        Err(TxTemplateError::NotFound)
    }
}
