use es_entity::*;
use sqlx::PgPool;

use super::entity::*;

#[derive(EsRepo, Debug, Clone)]
#[es_repo(
    entity = "Journal",
    columns(
        name(ty = "String", update(accessor = "values().name")),
        code(ty = "Option<String>", update(accessor = "values().code")),
    ),
    tbl_prefix = "cala",
    persist_event_context = false
)]
pub(super) struct JournalRepo {
    pool: PgPool,
}

impl JournalRepo {
    pub fn new(pool: &PgPool) -> Self {
        Self { pool: pool.clone() }
    }
}
