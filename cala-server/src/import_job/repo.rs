use sqlx::{PgPool, Postgres, Transaction};

#[derive(Debug, Clone)]
pub(super) struct ImportJobs {
    pool: PgPool,
}

impl ImportJobs {
    pub fn new(pool: &PgPool) -> Self {
        Self { pool: pool.clone() }
    }
}
