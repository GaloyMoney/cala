use sqlx::PgPool;

#[derive(Clone)]
pub(super) struct VelocityBalanceRepo {
    pool: PgPool,
}

impl VelocityBalanceRepo {
    pub fn new(pool: &PgPool) -> Self {
        Self { pool: pool.clone() }
    }

    pub async fn find_for_update(
        &self,
        // control_id: VelocityControlId,
        // limit_id: VelocityLimitId,
        // currency: Currency,
    ) -> Result<(), sqlx::Error> {
        unimplemented!()
    }
}
