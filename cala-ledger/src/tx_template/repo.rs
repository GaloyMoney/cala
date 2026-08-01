use cached::macros::cached;

use es_entity::*;
use sqlx::PgPool;
use tracing::instrument;

use std::sync::Arc;

use super::{entity::*, error::TxTemplateError};

#[derive(EsRepo, Clone)]
#[es_repo(
    entity = "TxTemplate",
    columns(code(
        ty = "String",
        update(accessor = "values().code", persist = false),
        list_by
    ),),
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

    #[instrument(
        level = "debug",
        name = "tx_template.find_latest_version_in_op",
        skip_all,
        err(level = "warn")
    )]
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
}

#[cached(
    key = "(TxTemplateId, i32)",
    convert = "{ (id, version) }",
    sync_writes = "default"
)]
#[instrument(
    level = "debug",
    name = "tx_template.find_versioned_cached",
    skip(op),
    fields(template_id = %id, version = version),
    err(level = "warn")
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
