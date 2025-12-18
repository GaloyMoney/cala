mod entity;
pub mod error;
mod repo;

use sqlx::PgPool;
use std::collections::HashMap;
use tracing::instrument;

#[cfg(feature = "import")]
use crate::primitives::DataSourceId;
use crate::{
    outbox::*,
    primitives::{AccountId, AccountSetId, DataSource, JournalId, TransactionId},
};

pub use entity::*;
use error::*;
pub use repo::entry_cursor::EntriesByCreatedAtCursor;
use repo::*;

#[derive(Clone)]
pub struct Entries {
    repo: EntryRepo,
}

impl Entries {
    pub(crate) fn new(pool: &PgPool, publisher: &OutboxPublisher) -> Self {
        Self {
            repo: EntryRepo::new(pool, publisher),
        }
    }

    #[instrument(name = "cala_ledger.entries.find_all", skip_all)]
    pub async fn find_all(
        &self,
        entry_ids: &[EntryId],
    ) -> Result<HashMap<EntryId, Entry>, EntryError> {
        self.repo.find_all(entry_ids).await
    }

    #[instrument(name = "cala_ledger.entries.list_for_account_id", skip_all)]
    pub async fn list_for_account_id(
        &self,
        account_id: AccountId,
        query: es_entity::PaginatedQueryArgs<EntriesByCreatedAtCursor>,
        direction: es_entity::ListDirection,
    ) -> Result<es_entity::PaginatedQueryRet<Entry, EntriesByCreatedAtCursor>, EntryError> {
        self.repo
            .list_for_account_id_by_created_at(account_id, query, direction)
            .await
    }

    #[instrument(name = "cala_ledger.entries.list_for_account_set_id", skip_all)]
    pub async fn list_for_account_set_id(
        &self,
        account_id: AccountSetId,
        query: es_entity::PaginatedQueryArgs<EntriesByCreatedAtCursor>,
        direction: es_entity::ListDirection,
    ) -> Result<es_entity::PaginatedQueryRet<Entry, EntriesByCreatedAtCursor>, EntryError> {
        self.repo
            .list_for_account_set_id_by_created_at(account_id, query, direction)
            .await
    }

    #[instrument(name = "cala_ledger.entries.list_for_journal_id", skip_all)]
    pub async fn list_for_journal_id(
        &self,
        journal_id: JournalId,
        query: es_entity::PaginatedQueryArgs<EntriesByCreatedAtCursor>,
        direction: es_entity::ListDirection,
    ) -> Result<es_entity::PaginatedQueryRet<Entry, EntriesByCreatedAtCursor>, EntryError> {
        self.repo
            .list_for_journal_id_by_created_at(journal_id, query, direction)
            .await
    }

    #[instrument(name = "cala_ledger.entries.list_for_transaction_id", skip_all)]
    pub async fn list_for_transaction_id(
        &self,
        transaction_id: TransactionId,
    ) -> Result<Vec<Entry>, EntryError> {
        let mut entries = self
            .repo
            .list_for_transaction_id_by_created_at(
                transaction_id,
                Default::default(),
                Default::default(),
            )
            .await?
            .entities;
        entries.sort_by(|a, b| {
            let a_sequence = a.values().sequence;
            let b_sequence = b.values().sequence;
            a_sequence.cmp(&b_sequence)
        });
        Ok(entries)
    }

    #[instrument(name = "cala_ledger.entries.new_entries_for_voided_tx", skip_all)]
    pub async fn new_entries_for_voided_tx(
        &self,
        voiding_tx_id: TransactionId,
        existing_tx_id: TransactionId,
    ) -> Result<Vec<NewEntry>, EntryError> {
        let entries = self.list_for_transaction_id(existing_tx_id).await?;

        let new_entries = entries
            .into_iter()
            .map(|entry| {
                let value = entry.into_values();

                let mut builder = NewEntry::builder();
                builder
                    .id(EntryId::new())
                    .transaction_id(voiding_tx_id)
                    .journal_id(value.journal_id)
                    .sequence(value.sequence)
                    .account_id(value.account_id)
                    .entry_type(format!("{}_VOID", value.entry_type))
                    .layer(value.layer)
                    .currency(value.currency)
                    .units(-value.units)
                    .direction(value.direction);

                if let Some(description) = value.description {
                    builder.description(description);
                }
                if let Some(metadata) = value.metadata {
                    builder.metadata(metadata);
                }

                builder.build().expect("Couldn't build voided entry")
            })
            .collect();

        Ok(new_entries)
    }

    #[instrument(name = "cala_ledger.entries.create_all_in_op", skip_all)]
    pub(crate) async fn create_all_in_op(
        &self,
        db: &mut impl es_entity::AtomicOperation,
        entries: Vec<NewEntry>,
    ) -> Result<Vec<EntryValues>, EntryError> {
        let entries = self.repo.create_all_in_op(db, entries).await?;
        Ok(entries
            .into_iter()
            .map(|entry| entry.into_values())
            .collect())
    }

    #[cfg(feature = "import")]
    #[instrument(name = "cala_ledger.entries.sync_entry_creation", skip_all)]
    pub(crate) async fn sync_entry_creation(
        &self,
        mut db: es_entity::DbOpWithTime<'_>,
        origin: DataSourceId,
        values: EntryValues,
    ) -> Result<(), EntryError> {
        let mut entry = Entry::import(origin, values);
        self.repo.import(&mut db, origin, &mut entry).await?;
        db.commit().await?;
        Ok(())
    }
}

impl From<&EntryEvent> for OutboxEventPayload {
    fn from(event: &EntryEvent) -> Self {
        let source = es_entity::context::EventContext::current()
            .data()
            .lookup("data_source")
            .ok()
            .flatten()
            .unwrap_or(DataSource::Local);

        match event {
            #[cfg(feature = "import")]
            EntryEvent::Imported {
                source,
                values: entry,
            } => OutboxEventPayload::EntryCreated {
                source: *source,
                entry: entry.clone(),
            },
            EntryEvent::Initialized { values: entry } => OutboxEventPayload::EntryCreated {
                source,
                entry: entry.clone(),
            },
        }
    }
}
