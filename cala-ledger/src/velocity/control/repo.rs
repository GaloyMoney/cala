use es_entity::*;
use sqlx::PgPool;

use crate::{primitives::DataSourceId, velocity::error::VelocityError};

use super::entity::*;

#[derive(EsRepo, Debug, Clone)]
#[es_repo(
    entity = "VelocityControl",
    err = "VelocityError",
    columns(
        name(ty = "String", update(persist = false)),
        data_source_id(
            ty = "DataSourceId",
            create(accessor = "data_source().into()"),
            update(persist = false)
        ),
    ),
    tbl_prefix = "cala"
)]
#[cfg_attr(not(feature = "event-context"), es_repo(event_context = false))]
pub struct VelocityControlRepo {
    pool: PgPool,
}

impl VelocityControlRepo {
    pub fn new(pool: &PgPool) -> Self {
        Self { pool: pool.clone() }
    }
}
