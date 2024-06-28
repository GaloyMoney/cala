pub mod error;
mod limit;

use sqlx::PgPool;

use crate::{atomic_operation::*, outbox::*};

use error::*;
pub use limit::*;

#[derive(Clone)]
pub struct Velocities {
    outbox: Outbox,
    pool: PgPool,
    limits: VelocityLimitRepo,
}

impl Velocities {
    pub(crate) fn new(pool: &PgPool, outbox: Outbox) -> Self {
        Self {
            limits: VelocityLimitRepo::new(pool),
            pool: pool.clone(),
            outbox,
        }
    }

    pub async fn create_limit(
        &self,
        new_limit: NewVelocityLimit,
    ) -> Result<VelocityLimit, VelocityError> {
        let mut op = AtomicOperation::init(&self.pool, &self.outbox).await?;
        let limit = self.create_limit_in_op(&mut op, new_limit).await?;
        op.commit().await?;
        Ok(limit)
    }

    pub async fn create_limit_in_op(
        &self,
        op: &mut AtomicOperation<'_>,
        new_limit: NewVelocityLimit,
    ) -> Result<VelocityLimit, VelocityError> {
        self.limits.create_in_tx(op.tx(), new_limit).await
    }
}
