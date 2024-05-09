mod cel_context;
mod entity;
mod repo;

pub mod error;

use sqlx::PgPool;
use tracing::instrument;

use crate::{entity::*, outbox::*};

pub use entity::*;
use error::*;
use repo::*;

#[derive(Clone)]
pub struct TxTemplates {
    repo: TxTemplateRepo,
    outbox: Outbox,
    pool: PgPool,
}

impl TxTemplates {
    pub fn new(pool: &PgPool, outbox: Outbox) -> Self {
        Self {
            repo: TxTemplateRepo::new(pool),
            outbox,
            pool: pool.clone(),
        }
    }

    #[instrument(name = "cala_ledger.accounts.create", skip(self))]
    pub async fn create(
        &self,
        new_tx_template: NewTxTemplate,
    ) -> Result<TxTemplate, TxTemplateError> {
        let mut tx = self.pool.begin().await?;
        let EntityUpdate {
            entity: tx_template,
            n_new_events,
        } = self.repo.create_in_tx(&mut tx, new_tx_template).await?;
        self.outbox
            .persist_events(tx, tx_template.events.last_persisted(n_new_events))
            .await?;
        Ok(tx_template)
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
