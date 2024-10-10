use cached::proc_macro::cached;
#[cfg(feature = "import")]
use chrono::{DateTime, Utc};
use sqlx::{PgPool, Postgres, Transaction};

use std::{collections::HashMap, sync::Arc};

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
        db: &mut Transaction<'_, Postgres>,
        new_tx_template: NewTxTemplate,
    ) -> Result<TxTemplate, TxTemplateError> {
        let id = new_tx_template.id;
        let created_at = new_tx_template.created_at;
        sqlx::query!(
            r#"INSERT INTO cala_tx_templates (id, created_at, code)
            VALUES ($1, $2, $3)"#,
            id as TxTemplateId,
            created_at,
            new_tx_template.code,
        )
        .execute(&mut **db)
        .await?;
        let mut events = new_tx_template.initial_events();
        events.persist_at(db, created_at).await?;
        let tx_template = TxTemplate::try_from(events)?;
        Ok(tx_template)
    }

    pub(super) async fn find_all<T: From<TxTemplate>>(
        &self,
        ids: &[TxTemplateId],
    ) -> Result<HashMap<TxTemplateId, T>, TxTemplateError> {
        let rows = sqlx::query_as!(
            GenericEvent,
            r#"SELECT t.id, e.sequence, e.event,
                t.created_at AS entity_created_at, e.recorded_at AS event_recorded_at
            FROM cala_tx_templates t
            JOIN cala_tx_template_events e
            ON t.data_source_id = e.data_source_id
            AND t.id = e.id
            WHERE t.data_source_id = '00000000-0000-0000-0000-000000000000'
            AND t.id = ANY($1)
            ORDER BY t.id, e.sequence"#,
            ids as &[TxTemplateId]
        )
        .fetch_all(&self.pool)
        .await?;
        let n = rows.len();
        let ret = EntityEvents::load_n(rows, n)?
            .0
            .into_iter()
            .map(|tx_template: TxTemplate| (tx_template.values().id, T::from(tx_template)))
            .collect();
        Ok(ret)
    }

    pub(super) async fn find_by_code(&self, code: &str) -> Result<TxTemplate, TxTemplateError> {
        let rows = sqlx::query_as!(
            GenericEvent,
            r#"SELECT a.id, e.sequence, e.event,
                a.created_at AS entity_created_at, e.recorded_at AS event_recorded_at
            FROM cala_tx_templates a
            JOIN cala_tx_template_events e
            ON a.data_source_id = e.data_source_id
            AND a.id = e.id
            WHERE a.data_source_id = '00000000-0000-0000-0000-000000000000'
            AND a.code = $1
            ORDER BY e.sequence"#,
            code
        )
        .fetch_all(&self.pool)
        .await?;
        match EntityEvents::load_first(rows) {
            Ok(tx_template) => Ok(tx_template),
            Err(EntityError::NoEntityEventsPresent) => {
                Err(TxTemplateError::CouldNotFindByCode(code.to_owned()))
            }
            Err(e) => Err(e.into()),
        }
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
        db: &mut Transaction<'_, Postgres>,
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
        .execute(&mut **db)
        .await?;
        tx_template
            .events
            .persisted_at(db, origin, recorded_at)
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
