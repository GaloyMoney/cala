use sqlx::{PgPool, Postgres, Transaction};

use super::{super::error::*, entity::*};
use crate::primitives::VelocityControlId;

#[derive(Debug, Clone)]
pub struct VelocityLimitRepo {
    _pool: PgPool,
}

impl VelocityLimitRepo {
    pub fn new(pool: &PgPool) -> Self {
        Self {
            _pool: pool.clone(),
        }
    }

    pub async fn create_in_tx(
        &self,
        db: &mut Transaction<'_, Postgres>,
        new_limit: NewVelocityLimit,
    ) -> Result<VelocityLimit, VelocityError> {
        let id = new_limit.id;
        sqlx::query!(
            r#"INSERT INTO cala_velocity_limits (id, name)
            VALUES ($1, $2)"#,
            id as VelocityLimitId,
            new_limit.name,
        )
        .execute(&mut **db)
        .await?;
        let mut events = new_limit.initial_events();
        events.persist(db).await?;
        let limit = VelocityLimit::try_from(events)?;
        Ok(limit)
    }

    pub async fn add_limit_to_control(
        &self,
        db: &mut Transaction<'_, Postgres>,
        control: VelocityControlId,
        limit: VelocityLimitId,
    ) -> Result<(), VelocityError> {
        sqlx::query!(
            r#"INSERT INTO cala_velocity_control_limits (velocity_control_id, velocity_limit_id)
            VALUES ($1, $2)"#,
            control as VelocityControlId,
            limit as VelocityLimitId,
        )
        .execute(&mut **db)
        .await?;
        Ok(())
    }

    pub async fn list_for_control(
        &self,
        db: &mut Transaction<'_, Postgres>,
        control: VelocityControlId,
    ) -> Result<Vec<VelocityLimitValues>, VelocityError> {
        let rows = sqlx::query_as!(
            GenericEvent,
            r#"WITH limits AS (
              SELECT id, l.data_source_id, l.created_at AS entity_created_at
              FROM cala_velocity_limits l
              JOIN cala_velocity_control_limits ON id = velocity_limit_id
              WHERE velocity_control_id = $1
              AND l.data_source_id = '00000000-0000-0000-0000-000000000000'
              AND l.data_source_id = cala_velocity_control_limits.data_source_id
            )
            SELECT l.id, e.sequence, e.event, entity_created_at, e.recorded_at AS event_recorded_at
            FROM limits l
            JOIN cala_velocity_limit_events e ON l.id = e.id
            WHERE l.data_source_id = e.data_source_id
            ORDER BY l.id, e.sequence"#,
            control as VelocityControlId,
        )
        .fetch_all(&mut **db)
        .await?;
        let n = rows.len();
        let ret = EntityEvents::load_n(rows, n)?
            .0
            .into_iter()
            .map(|l: VelocityLimit| l.into_values())
            .collect();
        Ok(ret)
    }
}
