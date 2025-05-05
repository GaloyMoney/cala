use sqlx::{Executor, PgPool, Postgres, QueryBuilder, Transaction};

#[derive(Debug, Clone)]
pub(super) struct EffectiveBalanceRepo {
    pool: PgPool,
}

impl EffectiveBalanceRepo {
    pub fn new(pool: &PgPool) -> Self {
        Self { pool: pool.clone() }
    }
}
