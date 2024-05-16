use sqlx::{PgPool, Postgres, Transaction};

#[derive(Debug, Clone)]
pub(super) struct BalanceRepo {
    _pool: PgPool,
}

impl BalanceRepo {
    pub fn new(pool: &PgPool) -> Self {
        Self {
            _pool: pool.clone(),
        }
    }
}
