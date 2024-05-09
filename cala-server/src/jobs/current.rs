use sqlx::PgPool;
use uuid::Uuid;

pub struct CurrentJob {
    id: Uuid,
    pool: PgPool,
}

impl CurrentJob {
    pub(super) fn new(id: Uuid, pool: PgPool) -> Self {
        Self { id, pool }
    }

    pub fn id(&self) -> Uuid {
        self.id
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }
}
