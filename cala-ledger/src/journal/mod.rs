mod entity;
pub mod error;
mod repo;

use es_entity::clock::ClockHandle;
use sqlx::PgPool;
use tracing::instrument;

use std::collections::HashMap;

use crate::outbox::*;

pub use entity::*;
use error::*;
use repo::*;

/// Service for working with `Journal` entities.
#[derive(Clone)]
pub struct Journals {
    repo: JournalRepo,
    clock: ClockHandle,
}

impl Journals {
    pub(crate) fn new(pool: &PgPool, publisher: &OutboxPublisher, clock: &ClockHandle) -> Self {
        Self {
            repo: JournalRepo::new(pool, publisher),
            clock: clock.clone(),
        }
    }

    #[instrument(name = "cala_ledger.journals.create", skip(self))]
    pub async fn create(&self, new_journal: NewJournal) -> Result<Journal, JournalError> {
        let mut op = self.repo.begin_op_with_clock(&self.clock).await?;
        let journal = self.create_in_op(&mut op, new_journal).await?;
        op.commit().await?;
        Ok(journal)
    }

    pub async fn create_in_op(
        &self,
        db: &mut impl es_entity::AtomicOperation,
        new_journal: NewJournal,
    ) -> Result<Journal, JournalError> {
        let journal = self.repo.create_in_op(db, new_journal).await?;
        Ok(journal)
    }

    #[instrument(name = "cala_ledger.journals.find_all", skip(self))]
    pub async fn find_all<T: From<Journal>>(
        &self,
        journal_ids: &[JournalId],
    ) -> Result<HashMap<JournalId, T>, JournalError> {
        self.repo.find_all(journal_ids).await
    }

    #[instrument(name = "cala_ledger.journals.find_by_id", skip(self))]
    pub async fn find(&self, journal_id: JournalId) -> Result<Journal, JournalError> {
        self.repo.find_by_id(journal_id).await
    }

    #[instrument(name = "cala_ledger.journals.persist", skip(self, journal))]
    pub async fn persist(&self, journal: &mut Journal) -> Result<(), JournalError> {
        let mut op = self.repo.begin_op_with_clock(&self.clock).await?;
        self.persist_in_op(&mut op, journal).await?;
        op.commit().await?;
        Ok(())
    }

    #[instrument(name = "cala_ledger.journals.persist_in_op", skip_all)]
    pub async fn persist_in_op(
        &self,
        db: &mut impl es_entity::AtomicOperation,
        journal: &mut Journal,
    ) -> Result<(), JournalError> {
        self.repo.update_in_op(db, journal).await?;
        Ok(())
    }

    #[instrument(name = "cala_ledger.journal.find_by_code", skip(self))]
    pub async fn find_by_code(&self, code: String) -> Result<Journal, JournalError> {
        self.repo.find_by_code(Some(code)).await
    }
}

use cala_types::balance::{JournalChecker, JournalInfo};

impl JournalChecker for Journals {
    type Error = JournalError;

    async fn check_journal(
        &self,
        journal_id: crate::primitives::JournalId,
    ) -> Result<JournalInfo, Self::Error> {
        let journal = self.find(journal_id).await?;
        Ok(JournalInfo {
            id: journal.id(),
            is_locked: journal.is_locked(),
            enable_effective_balances: journal.insert_effective_balances(),
        })
    }
}

impl From<&JournalEvent> for OutboxEventPayload {
    fn from(event: &JournalEvent) -> Self {
        match event {
            JournalEvent::Initialized { values } => OutboxEventPayload::JournalCreated {
                journal: values.clone(),
            },
            JournalEvent::Updated { values, fields } => OutboxEventPayload::JournalUpdated {
                journal: values.clone(),
                fields: fields.clone(),
            },
        }
    }
}
