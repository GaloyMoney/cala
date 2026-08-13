use sqlx::PgPool;
use tracing::instrument;

use crate::primitives::{AccountId, VelocityControlId};

use super::{super::error::*, value::*};

#[derive(Debug, Clone)]
pub struct AccountControlRepo {
    _pool: PgPool,
}

impl AccountControlRepo {
    pub fn new(pool: &PgPool) -> Self {
        Self {
            _pool: pool.clone(),
        }
    }

    #[instrument(
        level = "debug",
        name = "account_control.create_in_op",
        skip_all,
        err(level = "warn")
    )]
    pub async fn create_in_op(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        control: AccountVelocityControl,
    ) -> Result<(), VelocityError> {
        sqlx::query!(
            r#"INSERT INTO cala_velocity_account_controls (account_id, velocity_control_id, values)
            VALUES ($1, $2, $3)"#,
            control.account_id as AccountId,
            control.control_id as VelocityControlId,
            serde_json::to_value(control).expect("Failed to serialize control values"),
        )
        .execute(op.as_executor())
        .await?;
        Ok(())
    }
}
