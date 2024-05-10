mod cel_context;
mod entity;
mod repo;

pub mod error;

use sqlx::PgPool;
use tracing::instrument;

#[cfg(feature = "import")]
use crate::primitives::DataSourceId;
use crate::{entity::*, outbox::*, primitives::DataSource};

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

    #[cfg(feature = "import")]
    pub async fn sync_tx_template_creation(
        &self,
        mut tx: sqlx::Transaction<'_, sqlx::Postgres>,
        origin: DataSourceId,
        values: TxTemplateValues,
    ) -> Result<(), TxTemplateError> {
        let mut tx_template = TxTemplate::import(origin, values);
        self.repo.import(&mut tx, origin, &mut tx_template).await?;
        self.outbox
            .persist_events(tx, tx_template.events.last_persisted(1))
            .await?;
        Ok(())
    }
}

impl From<&TxTemplateEvent> for OutboxEventPayload {
    fn from(event: &TxTemplateEvent) -> Self {
        match event {
            #[cfg(feature = "import")]
            TxTemplateEvent::Imported { source, values } => OutboxEventPayload::TxTemplateCreated {
                source: *source,
                tx_template: values.clone(),
            },
            TxTemplateEvent::Initialized {
                values: tx_template,
            } => OutboxEventPayload::TxTemplateCreated {
                source: DataSource::Local,
                tx_template: tx_template.clone(),
            },
        }
    }
}
