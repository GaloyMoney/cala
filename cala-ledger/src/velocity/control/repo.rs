use es_entity::*;
use sqlx::PgPool;

use crate::velocity::error::VelocityError;

use super::entity::*;

#[derive(EsRepo, Debug, Clone)]
#[es_repo(
    entity = "VelocityControl",
    err = "VelocityError",
    columns(
        name(ty = "String", update(persist = false)),
    ),
    tbl_prefix = "cala",
    persist_event_context = false
)]
pub struct VelocityControlRepo {
    pool: PgPool,
}

impl VelocityControlRepo {
    pub fn new(pool: &PgPool) -> Self {
        Self { pool: pool.clone() }
    }
}
