use cached::proc_macro::cached;
use sqlx::{PgPool, Postgres, Transaction};

use std::sync::Arc;

#[cfg(feature = "import")]
use crate::primitives::DataSourceId;
use crate::{entity::*, primitives::DataSource};

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
        let n_new_events = events.persist(tx, DataSource::Local).await?;
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
            SELECT t.id AS "id: TxTemplateId", MAX(sequence) AS "version!" 
            FROM cala_tx_templates t
            JOIN cala_tx_template_events e ON t.id = e.id
            WHERE t.code = $1
            GROUP BY t.id"#,
            code,
        )
        .fetch_optional(&self.pool)
        .await?;
        if let Some(row) = row {
            find_versioned_template_cached(
                &self.pool,
                TxTemplateIdVersionCacheKey {
                    id: row.id,
                    version: row.version,
                },
            )
            .await
        } else {
            Err(TxTemplateError::NotFound)
        }
    }

    #[cfg(feature = "import")]
    pub async fn import(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        origin: DataSourceId,
        tx_template: &mut TxTemplate,
    ) -> Result<(), TxTemplateError> {
        sqlx::query!(
            r#"INSERT INTO cala_tx_templates (data_source_id, id, code)
            VALUES ($1, $2, $3)"#,
            origin as DataSourceId,
            tx_template.values().id as TxTemplateId,
            tx_template.values().code,
        )
        .execute(&mut **tx)
        .await?;
        tx_template.events.persist(tx, origin).await?;
        Ok(())
    }
}

#[derive(Clone, Copy, Eq, PartialEq, Hash)]
struct TxTemplateIdVersionCacheKey {
    id: TxTemplateId,
    version: i32,
}

#[cached(
    key = "TxTemplateIdVersionCacheKey",
    convert = r#"{ key }"#,
    result = true,
    sync_writes = true
)]
async fn find_versioned_template_cached(
    pool: &PgPool,
    key: TxTemplateIdVersionCacheKey,
) -> Result<Arc<TxTemplateValues>, TxTemplateError> {
    let row = sqlx::query!(
        r#"
          SELECT event 
          FROM cala_tx_template_events
          WHERE id = $1 AND sequence = $2"#,
        key.id as TxTemplateId,
        key.version as i32,
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
