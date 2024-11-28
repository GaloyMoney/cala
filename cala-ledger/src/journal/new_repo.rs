#[cfg(feature = "import")]
use chrono::{DateTime, Utc};
#[cfg(feature = "import")]
use es_entity::DbOp;
use es_entity::*;
use sqlx::PgPool;

use crate::primitives::DataSourceId;

use super::{error::JournalError, new_entity::*};

#[derive(EsRepo, Debug, Clone)]
#[es_repo(
    entity = "Journal",
    err = "JournalError",
    columns(
        name(ty = "String", update(accessor = "values().name")),
        data_source_id(
            ty = "DataSourceId",
            create(accessor = "data_source().into()"),
            update(persist = false),
            list_by = false
        ),
    ),
    tbl_prefix = "cala"
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
        op: &mut DbOp<'_>,
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
        .execute(&mut **op.tx())
        .await?;
        self.persist_events(op, &mut journal.events).await?;
        Ok(())
    }
}
