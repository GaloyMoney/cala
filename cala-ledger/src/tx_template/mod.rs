mod entity;
mod repo;

pub mod error;

use es_entity::clock::ClockHandle;
use sqlx::PgPool;
use std::collections::HashMap;
use tracing::instrument;

use crate::outbox::*;
pub use crate::param::*;

pub use entity::*;
use error::*;
pub use repo::tx_template_cursor::TxTemplateByCodeCursor;
use repo::*;

/// Template resolution and CEL evaluation for the posting hot path live in
/// [`crate::posting`], which caches template bodies per process and verifies
/// the version it used on the posting flow's own fence statement. This service
/// owns template CRUD and lookup.
#[derive(Clone)]
pub struct TxTemplates {
    repo: TxTemplateRepo,
    clock: ClockHandle,
}

impl TxTemplates {
    pub(crate) fn new(pool: &PgPool, publisher: &OutboxPublisher, clock: &ClockHandle) -> Self {
        Self {
            repo: TxTemplateRepo::new(pool, publisher),
            clock: clock.clone(),
        }
    }

    #[instrument(name = "cala_ledger.tx_template.create", skip(self))]
    pub async fn create(
        &self,
        new_tx_template: NewTxTemplate,
    ) -> Result<TxTemplate, TxTemplateError> {
        let mut op = self.repo.begin_op_with_clock(&self.clock).await?;
        let tx_template = self.create_in_op(&mut op, new_tx_template).await?;
        op.commit().await?;
        Ok(tx_template)
    }

    #[instrument(name = "cala_ledger.tx_template.create_in_op", skip(self, db))]
    pub async fn create_in_op(
        &self,
        db: &mut impl es_entity::AtomicOperation,
        new_tx_template: NewTxTemplate,
    ) -> Result<TxTemplate, TxTemplateError> {
        let tx_template = self.repo.create_in_op(db, new_tx_template).await?;
        Ok(tx_template)
    }

    #[instrument(level = "debug", name = "cala_ledger.tx_templates.find_all", skip(self, tx_template_ids), fields(tx_template_ids_count = tx_template_ids.len()))]
    pub async fn find_all<T: From<TxTemplate>>(
        &self,
        tx_template_ids: &[TxTemplateId],
    ) -> Result<HashMap<TxTemplateId, T>, TxTemplateError> {
        Ok(self.repo.find_all(tx_template_ids).await?)
    }

    #[instrument(level = "debug", name = "cala_ledger.tx_templates.list", skip(self))]
    pub async fn list(
        &self,
        cursor: es_entity::PaginatedQueryArgs<TxTemplateByCodeCursor>,
        direction: es_entity::ListDirection,
    ) -> Result<es_entity::PaginatedQueryRet<TxTemplate, TxTemplateByCodeCursor>, TxTemplateError>
    {
        Ok(self.repo.list_by_code(cursor, direction).await?)
    }

    #[instrument(level = "debug", name = "cala_ledger.tx_templates.find_by_code", skip(self), fields(code = %code.as_ref()), err(level = tracing::Level::WARN))]
    pub async fn find_by_code(&self, code: impl AsRef<str>) -> Result<TxTemplate, TxTemplateError> {
        Ok(self.repo.find_by_code(code.as_ref().to_string()).await?)
    }
}

impl From<&TxTemplateEvent> for OutboxEventPayload {
    fn from(event: &TxTemplateEvent) -> Self {
        match event {
            TxTemplateEvent::Initialized {
                values: tx_template,
            } => OutboxEventPayload::TxTemplateCreated {
                tx_template: tx_template.clone(),
            },
        }
    }
}
