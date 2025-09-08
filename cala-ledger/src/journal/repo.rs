use es_entity::*;
use sqlx::PgPool;

use crate::primitives::DataSourceId;

use super::{entity::*, error::JournalError};

#[derive(EsRepo, Debug, Clone)]
#[es_repo(
    entity = "Journal",
    err = "JournalError",
    columns(
        name(ty = "String", update(accessor = "values().name")),
        code(ty = "Option<String>", update(accessor = "values().code")),
        data_source_id(
            ty = "DataSourceId",
            create(accessor = "data_source().into()"),
            update(persist = false)
        ),
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

    #[cfg(feature = "import")]
    pub async fn import_in_op(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        origin: DataSourceId,
        journal: &mut Journal,
    ) -> Result<(), JournalError> {
        let recorded_at = op.now();
        sqlx::query!(
            r#"INSERT INTO cala_journals (data_source_id, id, name, created_at)
            VALUES ($1, $2, $3, $4)"#,
            origin as DataSourceId,
            journal.values().id as JournalId,
            journal.values().name,
            recorded_at
        )
        .execute(op.as_executor())
        .await?;
        self.persist_events(op, journal.events_mut()).await?;
        Ok(())
    }
}
