use cached::proc_macro::cached;

use es_entity::*;
use sqlx::PgPool;

use std::sync::Arc;

use crate::primitives::DataSourceId;

use super::{entity::*, error::TxTemplateError};

#[derive(EsRepo, Clone)]
#[es_repo(
    entity = "TxTemplate",
    err = "TxTemplateError",
    columns(
        code(
            ty = "String",
            update(accessor = "values().code", persist = false),
            list_by
        ),
        data_source_id(
            ty = "DataSourceId",
            create(accessor = "data_source().into()"),
            update(persist = false),
        ),
    ),
    tbl_prefix = "cala",
    persist_event_context = false
)]
pub(super) struct TxTemplateRepo {
    pool: PgPool,
}

impl TxTemplateRepo {
    pub fn new(pool: &PgPool) -> Self {
        Self { pool: pool.clone() }
    }

    pub async fn find_latest_version_in_op(
        &self,
        op: &mut impl es_entity::AtomicOperation,
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
        .fetch_optional(op.as_executor())
        .await?;
        if let Some(row) = row {
            if let (Some(id), Some(version)) = (row.id, row.version) {
                return find_versioned_template_cached(op, id, version).await;
            }
        }
        Err(TxTemplateError::NotFound)
    }

    #[cfg(feature = "import")]
    pub async fn import_in_op(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        origin: DataSourceId,
        tx_template: &mut TxTemplate,
    ) -> Result<(), TxTemplateError> {
        let recorded_at = op.now();
        sqlx::query!(
            r#"INSERT INTO cala_tx_templates (data_source_id, id, code, created_at)
            VALUES ($1, $2, $3, $4)"#,
            origin as DataSourceId,
            tx_template.values().id as TxTemplateId,
            tx_template.values().code,
            recorded_at
        )
        .execute(op.as_executor())
        .await?;
        self.persist_events(op, tx_template.events_mut()).await?;
        Ok(())
    }
}

#[cached(
    key = "(TxTemplateId, i32)",
    convert = "{ (id, version) }",
    result = true,
    sync_writes = "default"
)]
async fn find_versioned_template_cached(
    op: &mut impl es_entity::AtomicOperation,
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
    .fetch_optional(op.as_executor())
    .await?;
    if let Some(row) = row {
        let event: TxTemplateEvent = serde_json::from_value(row.event)?;
        Ok(Arc::new(event.into_values()))
    } else {
        Err(TxTemplateError::NotFound)
    }
}
