use sqlx::{PgPool, Postgres, Transaction};

use std::collections::HashMap;

use super::{super::error::*, entity::*};

#[derive(Debug, Clone)]
pub struct VelocityControlRepo {
    pool: PgPool,
}

impl VelocityControlRepo {
    pub fn new(pool: &PgPool) -> Self {
        Self { pool: pool.clone() }
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

    pub async fn find_all<T: From<VelocityControl>>(
        &self,
        ids: &[VelocityControlId],
    ) -> Result<HashMap<VelocityControlId, T>, VelocityError> {
        let rows = sqlx::query_as!(
            GenericEvent,
            r#"SELECT v.id, e.sequence, e.event,
                v.created_at AS entity_created_at, e.recorded_at AS event_recorded_at
            FROM cala_velocity_controls v
            JOIN cala_velocity_control_events e
            ON v.data_source_id = e.data_source_id
            AND v.id = e.id
            WHERE v.data_source_id = '00000000-0000-0000-0000-000000000000'
            AND v.id = ANY($1)
            ORDER BY v.id, e.sequence"#,
            ids as &[VelocityControlId]
        )
        .fetch_all(&self.pool)
        .await?;
        let n = rows.len();

        let ret = EntityEvents::load_n::<VelocityControl>(rows, n)?
            .0
            .into_iter()
            .map(|limit: VelocityControl| (limit.values().id, T::from(limit)))
            .collect();
        Ok(ret)
    }
}
