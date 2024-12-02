use crate::primitives::{AccountId, DataSourceId, EntryId, JournalId, TransactionId};
use es_entity::*;
use sqlx::PgPool;

use super::{entity::*, error::*};

#[derive(EsRepo, Debug, Clone)]
#[es_repo(
    entity = "Entry",
    err = "EntryError",
    columns(
        journal_id(ty = "JournalId", update(persist = false)),
        account_id(ty = "AccountId", update(persist = false)),
        transaction_id(ty = "TransactionId", update(persist = false)),
        data_source_id(
            ty = "DataSourceId",
            create(accessor = "data_source().into()"),
            update(persist = false),
        ),
    ),
    tbl_prefix = "cala"
)]
pub(crate) struct EntryRepo {
    #[allow(dead_code)]
    pool: PgPool,
}

impl EntryRepo {
    pub(crate) fn new(pool: &PgPool) -> Self {
        Self { pool: pool.clone() }
    }

    #[cfg(feature = "import")]
    pub(super) async fn import(
        &self,
        op: &mut DbOp<'_>,
        origin: DataSourceId,
        entry: &mut Entry,
    ) -> Result<(), EntryError> {
        let recorded_at = op.now();
        sqlx::query!(
            r#"INSERT INTO cala_entries (data_source_id, id, journal_id, account_id, created_at)
            VALUES ($1, $2, $3, $4, $5)"#,
            origin as DataSourceId,
            entry.values().id as EntryId,
            entry.values().journal_id as JournalId,
            entry.values().account_id as AccountId,
            recorded_at,
        )
        .execute(&mut **op.tx())
        .await?;
        self.persist_events(op, &mut entry.events).await?;
        Ok(())
    }
}
