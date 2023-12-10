use sqlx::PgPool;

#[derive(Clone)]
pub(super) struct OutboxRepo {
    pool: PgPool,
}

impl OutboxRepo {
    pub(super) fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}
