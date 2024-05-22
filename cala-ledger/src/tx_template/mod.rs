mod cel_context;
mod entity;
mod repo;
mod tx_params;

use chrono::NaiveDate;
#[cfg(feature = "import")]
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use sqlx::PgPool;
use std::collections::HashMap;
use tracing::instrument;
use uuid::Uuid;

use crate::{
    entity::*, entry::NewEntry, errors::*, outbox::*, primitives::DataSource, primitives::*,
    transaction::NewTransaction,
};

pub use entity::*;
use repo::*;
pub use tx_params::*;

pub(crate) struct PreparedTransaction {
    pub transaction: NewTransaction,
    pub entries: Vec<NewEntry>,
}

#[derive(Clone)]
pub struct TxTemplates {
    repo: TxTemplateRepo,
    outbox: Outbox,
    pool: PgPool,
}

impl TxTemplates {
    pub(crate) fn new(pool: &PgPool, outbox: Outbox) -> Self {
        Self {
            repo: TxTemplateRepo::new(pool),
            outbox,
            pool: pool.clone(),
        }
    }

    #[instrument(name = "cala_ledger.tx_template.create", skip(self))]
    pub async fn create(
        &self,
        new_tx_template: NewTxTemplate,
    ) -> Result<TxTemplate, OneOf<(EntityNotFound, HydratingEntityError, UnexpectedDbError)>> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| OneOf::new(UnexpectedDbError(e)))?;
        let EntityUpdate {
            entity: tx_template,
            n_new_events,
        } = self
            .repo
            .create_in_tx(&mut tx, new_tx_template)
            .await
            .map_err(OneOf::broaden)?;
        self.outbox
            .persist_events(tx, tx_template.events.last_persisted(n_new_events))
            .await
            .map_err(OneOf::broaden)?;
        Ok(tx_template)
    }

    #[instrument(name = "cala_ledger.tx_templates.find_all", skip(self), err)]
    pub async fn find_all(
        &self,
        tx_template_ids: &[TxTemplateId],
    ) -> Result<
        HashMap<TxTemplateId, TxTemplateValues>,
        OneOf<(HydratingEntityError, UnexpectedDbError)>,
    > {
        self.repo.find_all(tx_template_ids).await
    }

    pub async fn find_by_code(
        &self,
        code: String,
    ) -> Result<TxTemplate, OneOf<(EntityNotFound, HydratingEntityError, UnexpectedDbError)>> {
        self.repo.find_by_code(code).await
    }

    #[instrument(
        level = "trace",
        name = "cala_ledger.tx_template.prepare_transaction",
        skip(self)
    )]
    pub(crate) async fn prepare_transaction(
        &self,
        tx_id: TransactionId,
        code: &str,
        params: TxParams,
    ) -> Result<
        PreparedTransaction,
        OneOf<(
            UnbalancedTransaction,
            TxParamTypeMismatch,
            TooManyParams,
            CelEvaluationError,
            EntityNotFound,
            UnexpectedDbError,
        )>,
    > {
        let tmpl = self
            .repo
            .find_latest_version(code)
            .await
            .map_err(OneOf::broaden)?;

        let mut tx_builder = NewTransaction::builder();
        tx_builder.id(tx_id).tx_template_id(tmpl.id);

        let ctx = params
            .into_context(tmpl.params.as_ref())
            .map_err(OneOf::broaden)?;

        let journal_id: Uuid = tmpl
            .tx_input
            .journal_id
            .try_evaluate(&ctx)
            .map_err(|e| OneOf::new(CelEvaluationError(Box::new(e))))?;
        tx_builder.journal_id(journal_id);

        let effective: NaiveDate = tmpl
            .tx_input
            .effective
            .try_evaluate(&ctx)
            .map_err(|e| OneOf::new(CelEvaluationError(Box::new(e))))?;
        tx_builder.effective(effective);

        if let Some(correlation_id) = tmpl.tx_input.correlation_id.as_ref() {
            let correlation_id: String = correlation_id
                .try_evaluate(&ctx)
                .map_err(|e| OneOf::new(CelEvaluationError(Box::new(e))))?;
            tx_builder.correlation_id(correlation_id);
        }

        if let Some(external_id) = tmpl.tx_input.external_id.as_ref() {
            let external_id: String = external_id
                .try_evaluate(&ctx)
                .map_err(|e| OneOf::new(CelEvaluationError(Box::new(e))))?;
            tx_builder.external_id(external_id);
        }

        if let Some(description) = tmpl.tx_input.description.as_ref() {
            let description: String = description
                .try_evaluate(&ctx)
                .map_err(|e| OneOf::new(CelEvaluationError(Box::new(e))))?;
            tx_builder.description(description);
        }

        if let Some(metadata) = tmpl.tx_input.metadata.as_ref() {
            let metadata: serde_json::Value = metadata
                .try_evaluate(&ctx)
                .map_err(|e| OneOf::new(CelEvaluationError(Box::new(e))))?;
            tx_builder.metadata(metadata);
        }

        let tx = tx_builder.build().expect("tx_build should succeed");
        let entries = self
            .prep_entries(&tmpl, tx_id, JournalId::from(journal_id), ctx)
            .map_err(OneOf::broaden)?;

        Ok(PreparedTransaction {
            transaction: tx,
            entries,
        })
    }

    fn prep_entries(
        &self,
        tmpl: &TxTemplateValues,
        transaction_id: TransactionId,
        journal_id: JournalId,
        ctx: cel_interpreter::CelContext,
    ) -> Result<Vec<NewEntry>, OneOf<(UnbalancedTransaction, CelEvaluationError)>> {
        let mut new_entries = Vec::new();
        let mut totals = HashMap::new();
        for (zero_based_sequence, entry) in tmpl.entries.iter().enumerate() {
            let mut builder = NewEntry::builder();
            builder
                .id(EntryId::new())
                .transaction_id(transaction_id)
                .journal_id(journal_id)
                .sequence(zero_based_sequence as u32 + 1);
            let account_id: Uuid = entry
                .account_id
                .try_evaluate(&ctx)
                .map_err(|e| OneOf::new(CelEvaluationError(Box::new(e))))?;
            builder.account_id(account_id);

            let entry_type: String = entry
                .entry_type
                .try_evaluate(&ctx)
                .map_err(|e| OneOf::new(CelEvaluationError(Box::new(e))))?;
            builder.entry_type(entry_type);

            let layer: Layer = entry
                .layer
                .try_evaluate(&ctx)
                .map_err(|e| OneOf::new(CelEvaluationError(Box::new(e))))?;
            builder.layer(layer);

            let units: Decimal = entry
                .units
                .try_evaluate(&ctx)
                .map_err(|e| OneOf::new(CelEvaluationError(Box::new(e))))?;
            let currency: Currency = entry
                .currency
                .try_evaluate(&ctx)
                .map_err(|e| OneOf::new(CelEvaluationError(Box::new(e))))?;
            let direction: DebitOrCredit = entry
                .direction
                .try_evaluate(&ctx)
                .map_err(|e| OneOf::new(CelEvaluationError(Box::new(e))))?;

            let total = totals.entry((currency, layer)).or_insert(Decimal::ZERO);
            match direction {
                DebitOrCredit::Debit => *total -= units,
                DebitOrCredit::Credit => *total += units,
            };
            builder.units(units);
            builder.currency(currency);
            builder.direction(direction);

            if let Some(description) = entry.description.as_ref() {
                let description: String = description
                    .try_evaluate(&ctx)
                    .map_err(|e| OneOf::new(CelEvaluationError(Box::new(e))))?;
                builder.description(description);
            }

            new_entries.push(builder.build().expect("Couldn't build entry"));
        }

        for ((c, l), v) in totals {
            if v != Decimal::ZERO {
                return Err(OneOf::new(UnbalancedTransaction(c, l, v)));
            }
        }

        Ok(new_entries)
    }

    #[cfg(feature = "import")]
    pub async fn sync_tx_template_creation(
        &self,
        mut tx: sqlx::Transaction<'_, sqlx::Postgres>,
        recorded_at: DateTime<Utc>,
        origin: DataSourceId,
        values: TxTemplateValues,
    ) -> Result<(), OneOf<(UnexpectedDbError,)>> {
        let mut tx_template = TxTemplate::import(origin, values);
        self.repo
            .import(&mut tx, recorded_at, origin, &mut tx_template)
            .await?;
        self.outbox
            .persist_events_at(tx, tx_template.events.last_persisted(1), recorded_at)
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
