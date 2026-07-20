mod entity;
pub mod error;
mod repo;

use sqlx::PgPool;
use std::collections::HashMap;
use tracing::instrument;

use crate::{
    outbox::*,
    primitives::{AccountId, AccountSetId, JournalId, TransactionId},
};

pub use entity::*;
use error::*;
pub use repo::entry_cursor::EntryByCreatedAtCursor;
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
        Ok(self.repo.find_all(entry_ids).await?)
    }

    #[instrument(name = "cala_ledger.entries.list_for_account_id", skip_all)]
    pub async fn list_for_account_id(
        &self,
        account_id: AccountId,
        query: es_entity::PaginatedQueryArgs<EntryByCreatedAtCursor>,
        direction: es_entity::ListDirection,
    ) -> Result<es_entity::PaginatedQueryRet<Entry, EntryByCreatedAtCursor>, EntryError> {
        Ok(self
            .repo
            .list_for_account_id_by_created_at(account_id, query, direction)
            .await?)
    }

    #[instrument(name = "cala_ledger.entries.list_for_account_set_id", skip_all)]
    pub async fn list_for_account_set_id(
        &self,
        account_id: AccountSetId,
        query: es_entity::PaginatedQueryArgs<EntryByCreatedAtCursor>,
        direction: es_entity::ListDirection,
    ) -> Result<es_entity::PaginatedQueryRet<Entry, EntryByCreatedAtCursor>, EntryError> {
        self.repo
            .list_for_account_set_id_by_created_at(account_id, query, direction)
            .await
    }

    #[instrument(name = "cala_ledger.entries.list_for_journal_id", skip_all)]
    pub async fn list_for_journal_id(
        &self,
        journal_id: JournalId,
        query: es_entity::PaginatedQueryArgs<EntryByCreatedAtCursor>,
        direction: es_entity::ListDirection,
    ) -> Result<es_entity::PaginatedQueryRet<Entry, EntryByCreatedAtCursor>, EntryError> {
        Ok(self
            .repo
            .list_for_journal_id_by_created_at(journal_id, query, direction)
            .await?)
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
}

impl From<&EntryEvent> for OutboxEventPayload {
    fn from(event: &EntryEvent) -> Self {
        match event {
            EntryEvent::Initialized { values: entry } => OutboxEventPayload::EntryCreated {
                entry: entry.clone(),
            },
        }
    }
}
