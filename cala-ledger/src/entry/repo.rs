use sqlx::{PgPool, Postgres, Transaction};
use tracing::instrument;

use super::{entity::*, error::*};
#[cfg(feature = "import")]
use crate::primitives::DataSourceId;
use crate::primitives::{AccountId, EntryId, JournalId};

#[derive(Debug, Clone)]
pub(crate) struct EntryRepo {
    _pool: PgPool,
}

impl EntryRepo {
    pub(crate) fn new(pool: &PgPool) -> Self {
        Self {
            _pool: pool.clone(),
        }
    }

    #[instrument(
        level = "trace",
        name = "cala_ledger.entries.create_all",
        skip(self, tx)
    )]
    pub(crate) async fn create_all<'a>(
        &self,
        entries: Vec<NewEntry>,
        tx: &mut Transaction<'a, Postgres>,
    ) -> Result<Vec<EntryValues>, EntryError> {
        unimplemented!()
        // let mut query_builder: QueryBuilder<Postgres> = QueryBuilder::new(
        //     r#"WITH new_entries as (
        //          INSERT INTO cala_entries
        //           (id, journal_id, account_id, transaction_id)"#,
        // );
        // let mut partial_ret = HashMap::new();
        // let mut sequence = 1;
        // query_builder.push_values(
        //     entries,
        //     |mut builder,
        //      NewEntry {
        //          id,
        //          transaction_id,
        //          journal_id,
        //          account_id,
        //          entry_type,
        //          layer,
        //          units,
        //          currency,
        //          direction,
        //          description,
        //      }: NewEntry| {
        //         builder.push(id);
        //         builder.push_bind(transaction_id);
        //         builder.push_bind(journal_id);
        //         builder.push_bind(entry_type);
        //         builder.push_bind(layer);
        //         builder.push_bind(units);
        //         builder.push_bind(currency.code());
        //         builder.push_bind(direction);
        //         builder.push_bind(description);
        //         builder.push_bind(sequence);
        //         builder.push("(SELECT id FROM sqlx_ledger_accounts WHERE id = ");
        //         builder.push_bind_unseparated(account_id);
        //         builder.push_unseparated(")");
        //         partial_ret.insert(sequence, (account_id, units, currency, layer, direction));
        //         sequence += 1;
        //     },
        // );
        // query_builder.push(
        //     "RETURNING id, sequence, created_at ) SELECT * FROM new_entries ORDER BY sequence",
        // );
        // let query = query_builder.build();
        // let records = query.fetch_all(&mut **tx).await?;

        // let mut ret = Vec::new();
        // sequence = 1;
        // for r in records {
        //     let entry_id: Uuid = r.get("id");
        //     let created_at = r.get("created_at");
        //     let (account_id, units, currency, layer, direction) =
        //         partial_ret.remove(&sequence).expect("sequence not found");
        //     ret.push(StagedEntry {
        //         entry_id: entry_id.into(),
        //         account_id,
        //         units,
        //         currency,
        //         layer,
        //         direction,
        //         created_at,
        //     });
        //     sequence += 1;
        // }

        // Ok(ret)
    }

    #[cfg(feature = "import")]
    pub(super) async fn import(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        origin: DataSourceId,
        entry: &mut Entry,
    ) -> Result<(), EntryError> {
        sqlx::query!(
            r#"INSERT INTO cala_entries (data_source_id, id, journal_id, account_id)
            VALUES ($1, $2, $3, $4)"#,
            origin as DataSourceId,
            entry.values().id as EntryId,
            entry.values().journal_id as JournalId,
            entry.values().account_id as AccountId,
        )
        .execute(&mut **tx)
        .await?;
        entry.events.persist(tx, origin).await?;
        Ok(())
    }
}
