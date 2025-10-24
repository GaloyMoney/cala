use es_entity::{EntityEvents, GenericEvent, *};
use sqlx::PgPool;

use crate::{
    primitives::{DataSourceId, VelocityLimitId},
    velocity::error::VelocityError,
};

use super::entity::*;

#[derive(EsRepo, Debug, Clone)]
#[es_repo(
    entity = "VelocityLimit",
    err = "VelocityError",
    columns(
        name(ty = "String", update(persist = false)),
        data_source_id(
            ty = "DataSourceId",
            create(accessor = "data_source().into()"),
            update(persist = false)
        ),
    ),
    tbl_prefix = "cala",
    persist_event_context = false
)]
pub struct VelocityLimitRepo {
    pool: PgPool,
}

impl VelocityLimitRepo {
    pub fn new(pool: &PgPool) -> Self {
        Self { pool: pool.clone() }
    }

    #[tracing::instrument(
        name = "velocity_limit.add_limit_to_control",
        skip_all,
        err(level = "warn")
    )]
    pub async fn add_limit_to_control(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        control: VelocityControlId,
        limit: VelocityLimitId,
    ) -> Result<(), VelocityError> {
        sqlx::query!(
            r#"INSERT INTO cala_velocity_control_limits (velocity_control_id, velocity_limit_id)
            VALUES ($1, $2)"#,
            control as VelocityControlId,
            limit as VelocityLimitId,
        )
        .execute(op.as_executor())
        .await?;
        Ok(())
    }

    #[tracing::instrument(
        name = "velocity_limit.list_for_control",
        skip_all,
        err(level = "warn")
    )]
    pub async fn list_for_control(
        &self,
        op: impl es_entity::IntoOneTimeExecutor<'_>,
        control: VelocityControlId,
    ) -> Result<Vec<VelocityLimit>, VelocityError> {
        let rows = op
            .into_executor()
            .fetch_all(sqlx::query_as!(
                GenericEvent,
                r#"WITH limits AS (
              SELECT id, l.created_at AS entity_created_at
              FROM cala_velocity_limits l
              JOIN cala_velocity_control_limits ON id = velocity_limit_id
              WHERE velocity_control_id = $1
            )
            SELECT l.id as entity_id, e.sequence, e.event, NULL as "context: es_entity::ContextData", e.recorded_at
            FROM limits l
            JOIN cala_velocity_limit_events e ON l.id = e.id
            ORDER BY l.id, e.sequence"#,
                control as VelocityControlId,
            ))
            .await?;
        let n = rows.len();
        let ret = EntityEvents::load_n::<VelocityLimit>(rows, n)?.0;
        Ok(ret)
    }
}
