use sqlx::{PgPool, Postgres, Transaction};

use super::{super::error::*, entity::*};
use crate::primitives::VelocityLimitId;

#[derive(Debug, Clone)]
pub struct VelocityControlRepo {
    _pool: PgPool,
}

impl VelocityControlRepo {
    pub fn new(pool: &PgPool) -> Self {
        Self {
            _pool: pool.clone(),
        }
    }

    pub async fn create_in_tx(
        &self,
        db: &mut Transaction<'_, Postgres>,
        new_control: NewVelocityControl,
    ) -> Result<VelocityControl, VelocityError> {
        let id = new_control.id;
        sqlx::query!(
            r#"INSERT INTO cala_velocity_controls (id, name)
            VALUES ($1, $2)"#,
            id as VelocityControlId,
            new_control.name,
        )
        .execute(&mut **db)
        .await?;
        let mut events = new_control.initial_events();
        events.persist(db).await?;
        let control = VelocityControl::try_from(events)?;
        Ok(control)
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
}
