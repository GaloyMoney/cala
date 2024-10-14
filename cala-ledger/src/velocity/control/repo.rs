use sqlx::{PgPool, Postgres, Transaction};

use super::{super::error::*, entity::*};

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

    pub async fn find_by_id(
        &self,
        db: &mut Transaction<'_, Postgres>,
        id: VelocityControlId,
    ) -> Result<VelocityControl, VelocityError> {
        let rows = sqlx::query_as!(
            GenericEvent,
            r#"SELECT c.id, e.sequence, e.event,
                c.created_at AS entity_created_at, e.recorded_at AS event_recorded_at
            FROM cala_velocity_controls c
            JOIN cala_velocity_control_events e
            ON c.data_source_id = e.data_source_id
            AND c.id = e.id
            WHERE c.data_source_id = '00000000-0000-0000-0000-000000000000'
            AND c.id = $1
            ORDER BY e.sequence"#,
            id as VelocityControlId,
        )
        .fetch_all(&mut **db)
        .await?;
        match EntityEvents::load_first(rows) {
            Ok(account) => Ok(account),
            Err(EntityError::NoEntityEventsPresent) => {
                Err(VelocityError::CouldNotFindControlById(id))
            }
            Err(e) => Err(e.into()),
        }
    }
}
