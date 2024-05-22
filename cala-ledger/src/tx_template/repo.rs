use cached::proc_macro::cached;
#[cfg(feature = "import")]
use chrono::{DateTime, Utc};
use sqlx::{PgPool, Postgres, QueryBuilder, Transaction};
use terrors::OneOf;

use std::{collections::HashMap, sync::Arc};

#[cfg(feature = "import")]
use crate::primitives::DataSourceId;
use crate::{entity::*, errors::*};

use super::entity::*;

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
    ) -> Result<EntityUpdate<TxTemplate>, OneOf<(UnexpectedDbError,)>> {
        let id = new_tx_template.id;
        sqlx::query!(
            r#"INSERT INTO cala_tx_templates (id, code)
            VALUES ($1, $2)"#,
            id as TxTemplateId,
            new_tx_template.code,
        )
        .execute(&mut **tx)
        .await
        .map_err(UnexpectedDbError)?;
        let mut events = new_tx_template.initial_events();
        let n_new_events = events.persist(tx).await?;
        let tx_template = TxTemplate::try_from(events).expect("Couldn't hydrate new entity");
        Ok(EntityUpdate {
            entity: tx_template,
            n_new_events,
        })
    }

    pub(super) async fn find_all(
        &self,
        ids: &[TxTemplateId],
    ) -> Result<
        HashMap<TxTemplateId, TxTemplateValues>,
        OneOf<(HydratingEntityError, UnexpectedDbError)>,
    > {
        let mut query_builder = QueryBuilder::new(
            r#"SELECT a.id, e.sequence, e.event,
                a.created_at AS entity_created_at, e.recorded_at AS event_recorded_at
            FROM cala_tx_templates a
            JOIN cala_tx_template_events e
            ON a.data_source_id = e.data_source_id
            AND a.id = e.id
            WHERE a.data_source_id = '00000000-0000-0000-0000-000000000000'
            AND a.id IN"#,
        );
        query_builder.push_tuples(ids, |mut builder, tx_template_id| {
            builder.push_bind(tx_template_id);
        });
        query_builder.push(r#"ORDER BY a.id, e.sequence"#);
        let query = query_builder.build_query_as::<GenericEvent>();
        let rows = query
            .fetch_all(&self.pool)
            .await
            .map_err(|e| OneOf::new(UnexpectedDbError(e)))?;
        let n = rows.len();
        let ret = EntityEvents::load_n(rows, n)
            .map_err(OneOf::broaden)?
            .0
            .into_iter()
            .map(|tx_template: TxTemplate| (tx_template.values().id, tx_template.into_values()))
            .collect();
        Ok(ret)
    }

    pub(super) async fn find_by_code(
        &self,
        code: String,
    ) -> Result<TxTemplate, OneOf<(EntityNotFound, HydratingEntityError, UnexpectedDbError)>> {
        let rows = sqlx::query_as!(
            GenericEvent,
            r#"SELECT a.id, e.sequence, e.event,
                a.created_at AS entity_created_at, e.recorded_at AS event_recorded_at
            FROM cala_tx_templates a
            JOIN cala_tx_template_events e
            ON a.data_source_id = e.data_source_id
            AND a.id = e.id
            WHERE a.data_source_id = '00000000-0000-0000-0000-000000000000'
            AND a.code = $1"#,
            code
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| OneOf::new(UnexpectedDbError(e)))?;
        Ok(EntityEvents::load_first(rows).map_err(OneOf::broaden)?)
    }

    pub async fn find_latest_version(
        &self,
        code: &str,
    ) -> Result<Arc<TxTemplateValues>, OneOf<(EntityNotFound, UnexpectedDbError)>> {
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
        .await
        .map_err(|e| OneOf::new(UnexpectedDbError(e)))?;
        if let Some((id, version)) = row.and_then(|row| {
            row.id
                .and_then(|id| row.version.map(|version| (id, version)))
        }) {
            find_versioned_template_cached(&self.pool, id, version).await
        } else {
            Err(OneOf::new(EntityNotFound))
        }
    }

    #[cfg(feature = "import")]
    pub async fn import(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        recorded_at: DateTime<Utc>,
        origin: DataSourceId,
        tx_template: &mut TxTemplate,
    ) -> Result<(), OneOf<(UnexpectedDbError,)>> {
        sqlx::query!(
            r#"INSERT INTO cala_tx_templates (data_source_id, id, code, created_at)
            VALUES ($1, $2, $3, $4)"#,
            origin as DataSourceId,
            tx_template.values().id as TxTemplateId,
            tx_template.values().code,
            recorded_at
        )
        .execute(&mut **tx)
        .await
        .map_err(|e| OneOf::new(UnexpectedDbError(e)))?;
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
) -> Result<Arc<TxTemplateValues>, OneOf<(EntityNotFound, UnexpectedDbError)>> {
    let row = sqlx::query!(
        r#"
          SELECT event 
          FROM cala_tx_template_events
          WHERE id = $1 AND sequence = $2"#,
        id as TxTemplateId,
        version,
    )
    .fetch_optional(pool)
    .await
    .map_err(|e| OneOf::new(UnexpectedDbError(e)))?;
    let event = row.ok_or_else(|| OneOf::new(EntityNotFound))?.event;
    let event: TxTemplateEvent =
        serde_json::from_value(event).expect("Could not deserialize event");
    Ok(Arc::new(event.into_values()))
}
