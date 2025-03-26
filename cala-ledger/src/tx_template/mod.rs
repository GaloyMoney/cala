mod entity;
mod repo;

pub mod error;

use chrono::{DateTime, NaiveDate, Utc};
use rust_decimal::Decimal;
use sqlx::PgPool;
use std::collections::HashMap;
use tracing::instrument;
use uuid::Uuid;

pub use crate::param::*;
use crate::{
    entry::NewEntry,
    ledger_operation::*,
    outbox::*,
    primitives::{DataSource, *},
    transaction::NewTransaction,
};

pub use entity::*;
use error::*;
use repo::*;

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
    ) -> Result<TxTemplate, TxTemplateError> {
        let mut op = LedgerOperation::init(&self.pool, &self.outbox).await?;
        let tx_template = self.create_in_op(&mut op, new_tx_template).await?;
        op.commit().await?;
        Ok(tx_template)
    }

    pub async fn create_in_op(
        &self,
        db: &mut LedgerOperation<'_>,
        new_tx_template: NewTxTemplate,
    ) -> Result<TxTemplate, TxTemplateError> {
        let tx_template = self.repo.create_in_op(db.op(), new_tx_template).await?;
        db.accumulate(tx_template.events.last_persisted(1).map(|p| &p.event));
        Ok(tx_template)
    }

    #[instrument(name = "cala_ledger.tx_templates.find_all", skip(self), err)]
    pub async fn find_all<T: From<TxTemplate>>(
        &self,
        tx_template_ids: &[TxTemplateId],
    ) -> Result<HashMap<TxTemplateId, T>, TxTemplateError> {
        self.repo.find_all(tx_template_ids).await
    }

    pub async fn find_by_code(&self, code: impl AsRef<str>) -> Result<TxTemplate, TxTemplateError> {
        self.repo.find_by_code(code.as_ref().to_string()).await
    }

    #[instrument(
        level = "trace",
        name = "cala_ledger.tx_template.prepare_transaction",
        skip(self)
    )]
    pub(crate) async fn prepare_transaction(
        &self,
        time: DateTime<Utc>,
        tx_id: TransactionId,
        code: &str,
        params: Params,
    ) -> Result<PreparedTransaction, TxTemplateError> {
        let tmpl = self.repo.find_latest_version(code).await?;

        let ctx = params.into_context(tmpl.params.as_ref())?;

        let journal_id: Uuid = tmpl.transaction.journal_id.try_evaluate(&ctx)?;

        let entries = self.prep_entries(&tmpl, tx_id, JournalId::from(journal_id), &ctx)?;

        let mut tx_builder = NewTransaction::builder();
        tx_builder
            .id(tx_id)
            .created_at(time)
            .tx_template_id(tmpl.id)
            .entry_ids(entries.iter().map(|e| e.id).collect());

        tx_builder.journal_id(journal_id);

        let effective: NaiveDate = tmpl.transaction.effective.try_evaluate(&ctx)?;
        tx_builder.effective(effective);

        if let Some(correlation_id) = tmpl.transaction.correlation_id.as_ref() {
            let correlation_id: String = correlation_id.try_evaluate(&ctx)?;
            tx_builder.correlation_id(correlation_id);
        }

        if let Some(external_id) = tmpl.transaction.external_id.as_ref() {
            let external_id: String = external_id.try_evaluate(&ctx)?;
            tx_builder.external_id(external_id);
        }

        if let Some(description) = tmpl.transaction.description.as_ref() {
            let description: String = description.try_evaluate(&ctx)?;
            tx_builder.description(description);
        }

        if let Some(metadata) = tmpl.transaction.metadata.as_ref() {
            let metadata: serde_json::Value = metadata.try_evaluate(&ctx)?;
            tx_builder.metadata(metadata);
        }

        let tx = tx_builder.build().expect("tx_build should succeed");

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
        ctx: &cel_interpreter::CelContext,
    ) -> Result<Vec<NewEntry>, TxTemplateError> {
        let mut new_entries = Vec::new();
        let mut totals = HashMap::new();
        for (zero_based_sequence, entry) in tmpl.entries.iter().enumerate() {
            let mut builder = NewEntry::builder();
            builder
                .id(EntryId::new())
                .transaction_id(transaction_id)
                .journal_id(journal_id)
                .sequence(zero_based_sequence as u32 + 1);
            let account_id: Uuid = entry.account_id.try_evaluate(ctx)?;
            builder.account_id(account_id);

            let entry_type: String = entry.entry_type.try_evaluate(ctx)?;
            builder.entry_type(entry_type);

            let layer: Layer = entry.layer.try_evaluate(ctx)?;
            builder.layer(layer);

            let units: Decimal = entry.units.try_evaluate(ctx)?;
            let currency: Currency = entry.currency.try_evaluate(ctx)?;
            let direction: DebitOrCredit = entry.direction.try_evaluate(ctx)?;

            let total = totals.entry((currency, layer)).or_insert(Decimal::ZERO);
            match direction {
                DebitOrCredit::Debit => *total -= units,
                DebitOrCredit::Credit => *total += units,
            };
            builder.units(units);
            builder.currency(currency);
            builder.direction(direction);

            if let Some(description) = entry.description.as_ref() {
                let description: String = description.try_evaluate(ctx)?;
                builder.description(description);
            }

            if let Some(metadata) = entry.metadata.as_ref() {
                let metadata: serde_json::Value = metadata.try_evaluate(ctx)?;
                builder.metadata(metadata);
            }

            new_entries.push(builder.build().expect("Couldn't build entry"));
        }

        for ((c, l), v) in totals {
            if v != Decimal::ZERO {
                return Err(TxTemplateError::UnbalancedTransaction(c, l, v));
            }
        }

        Ok(new_entries)
    }

    #[cfg(feature = "import")]
    pub async fn sync_tx_template_creation(
        &self,
        mut db: es_entity::DbOp<'_>,
        origin: DataSourceId,
        values: TxTemplateValues,
    ) -> Result<(), TxTemplateError> {
        let mut tx_template = TxTemplate::import(origin, values);
        self.repo
            .import_in_op(&mut db, origin, &mut tx_template)
            .await?;
        let recorded_at = db.now();
        let outbox_events: Vec<_> = tx_template
            .events
            .last_persisted(1)
            .map(|p| OutboxEventPayload::from(&p.event))
            .collect();
        self.outbox
            .persist_events_at(db.into_tx(), outbox_events, recorded_at)
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
