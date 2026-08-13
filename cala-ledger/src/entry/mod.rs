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
    pub(crate) fn new(pool: &PgPool) -> Self {
        Self {
            repo: EntryRepo::new(pool),
        }
    }

    #[instrument(level = "debug", name = "cala_ledger.entries.find_all", skip_all)]
    pub async fn find_all(
        &self,
        entry_ids: &[EntryId],
    ) -> Result<HashMap<EntryId, Entry>, EntryError> {
        Ok(self.repo.find_all(entry_ids).await?)
    }

    #[instrument(level = "debug", name = "cala_ledger.entries.find_all_in_op", skip_all)]
    pub(crate) async fn find_all_in_op(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        entry_ids: &[EntryId],
    ) -> Result<HashMap<EntryId, Entry>, EntryError> {
        Ok(self.repo.find_all_in_op(op, entry_ids).await?)
    }

    #[instrument(
        level = "debug",
        name = "cala_ledger.entries.list_for_account_id",
        skip_all
    )]
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

    #[instrument(
        level = "debug",
        name = "cala_ledger.entries.list_for_account_set_id",
        skip_all
    )]
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

    #[instrument(
        level = "debug",
        name = "cala_ledger.entries.list_for_journal_id",
        skip_all
    )]
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

    /// List a journal's entries with optional inclusive filters on the entry
    /// creation time and on the posting transaction's effective date, paginated
    /// on `(created_at, id)`. Unlike [`Self::list_for_journal_id`] this supports
    /// date ranges and the cross-entity effective-date filter, which the generated
    /// `list_for_*` methods cannot express.
    #[instrument(
        level = "debug",
        name = "cala_ledger.entries.list_for_journal_id_filtered",
        skip_all
    )]
    pub async fn list_for_journal_id_filtered(
        &self,
        journal_id: JournalId,
        filter: EntriesFilter,
        query: es_entity::PaginatedQueryArgs<EntryByCreatedAtCursor>,
        direction: es_entity::ListDirection,
    ) -> Result<es_entity::PaginatedQueryRet<Entry, EntryByCreatedAtCursor>, EntryError> {
        self.repo
            .list_for_journal_id_filtered_by_created_at(journal_id, filter, query, direction)
            .await
    }

    #[instrument(
        level = "debug",
        name = "cala_ledger.entries.list_for_transaction_id",
        skip_all
    )]
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
