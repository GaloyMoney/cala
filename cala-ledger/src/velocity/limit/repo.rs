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
    tbl_prefix = "cala"
)]
pub struct VelocityLimitRepo {
    pool: PgPool,
}

impl VelocityLimitRepo {
    pub fn new(pool: &PgPool) -> Self {
        Self { pool: pool.clone() }
    }

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

    pub async fn list_for_control(
        &self,
        op: impl es_entity::IntoExecutor<'_>,
        control: VelocityControlId,
    ) -> Result<Vec<VelocityLimit>, VelocityError> {
        let rows = sqlx::query_as!(
            GenericEvent,
            r#"WITH limits AS (
              SELECT id, l.created_at AS entity_created_at
              FROM cala_velocity_limits l
              JOIN cala_velocity_control_limits ON id = velocity_limit_id
              WHERE velocity_control_id = $1
            )
            SELECT l.id as entity_id, e.sequence, e.event, e.recorded_at
            FROM limits l
            JOIN cala_velocity_limit_events e ON l.id = e.id
            ORDER BY l.id, e.sequence"#,
            control as VelocityControlId,
        )
        .fetch_all(op.into_executor())
        .await?;
        let n = rows.len();
        let ret = EntityEvents::load_n::<VelocityLimit>(rows, n)?.0;
        Ok(ret)
    }
}
