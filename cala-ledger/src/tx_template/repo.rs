use cached::proc_macro::cached;
#[cfg(feature = "import")]
use es_entity::DbOp;

use es_entity::*;
use sqlx::PgPool;

use std::sync::Arc;

#[cfg(feature = "import")]
use crate::primitives::DataSourceId;

use super::{entity::*, error::TxTemplateError};


#[derive(EsRepo, Clone)]
#[es_repo(
    entity = "TxTemplate",
    err = "TxTemplateError",
    columns(
        code(ty = "String", update(persist = false), list_by = false),
        data_source_id(
            ty = "DataSourceId",
            create(accessor = "data_source().into()"),
            update(persist = false),
            list_by = false
        ),
    ),
    tbl_prefix = "cala"
)]
pub(super) struct TxTemplateRepo {
    pool: PgPool,
}

impl TxTemplateRepo {
    pub fn new(pool: &PgPool) -> Self {
        Self { pool: pool.clone() }
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
    pub async fn import_in_op(
        &self,
        op: &mut DbOp<'_>,
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
        .execute(&mut **op.tx())
        .await?;
        self.persist_events(op, &mut tx_template.events).await?;
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
