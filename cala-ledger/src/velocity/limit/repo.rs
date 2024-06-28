use sqlx::{PgPool, Postgres, Transaction};

use super::{super::error::*, entity::*};

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
}
