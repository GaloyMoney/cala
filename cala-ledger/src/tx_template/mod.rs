use crate::error::CalaFault;
use crate::posting::{PostingRef, PreparePostingRejection};
use es_entity::errlanes::{lanes, Fail, ResultExt};
mod entity;
mod repo;

pub mod error;

use chrono::NaiveDate;
use es_entity::clock::ClockHandle;
use rust_decimal::Decimal;
use sqlx::PgPool;
use std::collections::HashMap;
use uuid::Uuid;

use crate::outbox::*;
pub use crate::param::*;
use crate::{entry::NewEntry, primitives::*, transaction::NewTransaction};

pub use entity::*;
use error::*;
pub use repo::tx_template_cursor::TxTemplateByCodeCursor;
use repo::*;

/// A transaction evaluated down to the rows it will write.
///
/// `created_at` is deliberately absent: the posting flow pins the transaction
/// timestamp on its fence statement *after* preparation, so the
/// `NewTransaction` is only built once that timestamp is known
/// ([`Self::into_new_transaction`]). Everything that determines the flow's
/// lock set — the entry accounts and currencies — is already fixed here.
pub(crate) struct PreparedTransaction {
    pub(crate) tx_id: TransactionId,
    pub(crate) journal_id: JournalId,
    pub(crate) tx_template_id: TxTemplateId,
    pub(crate) effective: NaiveDate,
    pub(crate) correlation_id: Option<String>,
    pub(crate) external_id: Option<String>,
    pub(crate) description: Option<String>,
    pub(crate) metadata: Option<serde_json::Value>,
    pub(crate) entries: Vec<NewEntry>,
}

impl PreparedTransaction {
    pub(crate) fn into_new_transaction(
        self,
        created_at: chrono::DateTime<chrono::Utc>,
    ) -> (NewTransaction, Vec<NewEntry>) {
        let mut builder = NewTransaction::builder();
        builder
            .id(self.tx_id)
            .created_at(created_at)
            .journal_id(self.journal_id)
            .tx_template_id(self.tx_template_id)
            .effective(self.effective)
            .entry_ids(self.entries.iter().map(|e| e.id).collect());
        if let Some(correlation_id) = self.correlation_id {
            builder.correlation_id(correlation_id);
        }
        if let Some(external_id) = self.external_id {
            builder.external_id(external_id);
        }
        if let Some(description) = self.description {
            builder.description(description);
        }
        if let Some(metadata) = self.metadata {
            builder.metadata(Some(metadata));
        }
        (
            builder.build().expect("tx_build should succeed"),
            self.entries,
        )
    }
}

/// This service owns template CRUD, lookup — and the CEL evaluation that turns
/// a template body plus params into a [`PreparedTransaction`]
/// ([`Self::prepare_transaction`]). Fetching the body on the hot path is
/// [`crate::posting`]'s concern: it caches bodies per process and verifies the
/// version it used on the posting flow's own fence statement.
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

    #[es_entity::errlanes::instrument(name = "cala_ledger.tx_template.create", skip(self))]
    pub async fn create(
        &self,
        new_tx_template: NewTxTemplate,
    ) -> Result<TxTemplate, Fail<CreateTxTemplateRejection, lanes!(Transient, Fatal)>> {
        let mut op = self.repo.begin_op_with_clock(&self.clock).await?;
        let tx_template = self.create_in_op(&mut op, new_tx_template).await?;
        op.commit().await?;
        Ok(tx_template)
    }

    #[es_entity::errlanes::instrument(
        name = "cala_ledger.tx_template.create_in_op",
        skip(self, db)
    )]
    pub async fn create_in_op(
        &self,
        db: &mut impl es_entity::AtomicOperation,
        new_tx_template: NewTxTemplate,
    ) -> Result<TxTemplate, Fail<CreateTxTemplateRejection, lanes!(Transient, Fatal)>> {
        let tx_template = self.repo.create_in_op(db, new_tx_template).await.widen()?;
        Ok(tx_template)
    }

    #[es_entity::errlanes::instrument(level = "debug", name = "cala_ledger.tx_templates.find_all", skip(self, tx_template_ids), fields(tx_template_ids_count = tx_template_ids.len()))]
    pub async fn find_all<T: From<TxTemplate>>(
        &self,
        tx_template_ids: &[TxTemplateId],
    ) -> Result<HashMap<TxTemplateId, T>, CalaFault> {
        self.repo.find_all(tx_template_ids).await
    }

    #[es_entity::errlanes::instrument(
        level = "debug",
        name = "cala_ledger.tx_templates.list",
        skip(self)
    )]
    pub async fn list(
        &self,
        cursor: es_entity::PaginatedQueryArgs<TxTemplateByCodeCursor>,
        direction: es_entity::ListDirection,
    ) -> Result<es_entity::PaginatedQueryRet<TxTemplate, TxTemplateByCodeCursor>, CalaFault> {
        self.repo.list_by_code(cursor, direction).await
    }

    #[es_entity::errlanes::instrument(level = "debug", name = "cala_ledger.tx_templates.find_by_code", skip(self), fields(code = %code.as_ref()))]
    pub async fn find_by_code(
        &self,
        code: impl AsRef<str>,
    ) -> Result<TxTemplate, Fail<TxTemplateNotFound, lanes!(Transient, Fatal)>> {
        Ok(self
            .repo
            .maybe_find_by_code(code.as_ref().to_string())
            .await?
            .ok_or(TxTemplateNotFound(code.as_ref().to_string()))?)
    }

    /// Evaluate a template body against its params.
    ///
    /// Pure: no clock read that matters to persistence, no database access —
    /// which is what lets the posting flow run it before its first statement.
    /// The clock only seeds the CEL context (the `date()`/`now()` builtins
    /// available to template expressions).
    #[tracing::instrument(
        level = "debug",
        name = "cala_ledger.tx_template.prepare_transaction",
        skip(self, tmpl, params)
    )]
    pub(crate) fn prepare_transaction(
        &self,
        posting: PostingRef,
        tmpl: &TxTemplateValues,
        params: Params,
    ) -> Result<PreparedTransaction, PreparePostingRejection> {
        let tx_id = posting.tx_id;
        let ctx = params.into_context(
            &self.clock,
            tmpl.params.as_ref(),
            |parameter, source| PreparePostingRejection::Param {
                posting,
                parameter: parameter.to_owned(),
                source: Box::new(source),
            },
            |parameter, source| PreparePostingRejection::Default {
                posting,
                parameter: parameter.to_owned(),
                source: Box::new(source),
            },
        )?;

        let journal_id: Uuid =
            tmpl.transaction
                .journal_id
                .try_evaluate(&ctx)
                .map_err(|source| PreparePostingRejection::Cel {
                    posting,
                    source: Box::new(source),
                })?;
        let journal_id = JournalId::from(journal_id);
        let entries = self.prep_entries(tmpl, posting, journal_id, &ctx)?;
        let effective: NaiveDate =
            tmpl.transaction
                .effective
                .try_evaluate(&ctx)
                .map_err(|source| PreparePostingRejection::Cel {
                    posting,
                    source: Box::new(source),
                })?;

        let correlation_id = tmpl
            .transaction
            .correlation_id
            .as_ref()
            .map(|e| e.try_evaluate(&ctx))
            .transpose()
            .map_err(|source| PreparePostingRejection::Cel {
                posting,
                source: Box::new(source),
            })?;
        let external_id = tmpl
            .transaction
            .external_id
            .as_ref()
            .map(|e| e.try_evaluate(&ctx))
            .transpose()
            .map_err(|source| PreparePostingRejection::Cel {
                posting,
                source: Box::new(source),
            })?;
        let description = tmpl
            .transaction
            .description
            .as_ref()
            .map(|e| e.try_evaluate(&ctx))
            .transpose()
            .map_err(|source| PreparePostingRejection::Cel {
                posting,
                source: Box::new(source),
            })?;
        let metadata = tmpl
            .transaction
            .metadata
            .as_ref()
            .map(|e| e.try_evaluate(&ctx))
            .transpose()
            .map_err(|source| PreparePostingRejection::Cel {
                posting,
                source: Box::new(source),
            })?;

        Ok(PreparedTransaction {
            tx_id,
            journal_id,
            tx_template_id: tmpl.id,
            effective,
            correlation_id,
            external_id,
            description,
            metadata,
            entries,
        })
    }

    #[tracing::instrument(
        level = "debug",
        name = "tx_template.prep_entries",
        skip(self, tmpl, ctx),
        fields(
            template_id = %tmpl.id,
            template_code = %tmpl.code,
            transaction_id = %posting.tx_id,
            journal_id = %journal_id,
            entries_count = tmpl.entries.len()
        ),
    )]
    fn prep_entries(
        &self,
        tmpl: &TxTemplateValues,
        posting: PostingRef,
        journal_id: JournalId,
        ctx: &cel_interpreter::CelContext,
    ) -> Result<Vec<NewEntry>, PreparePostingRejection> {
        let transaction_id = posting.tx_id;
        let mut new_entries = Vec::with_capacity(tmpl.entries.len());
        let mut totals = HashMap::new();
        for (zero_based_sequence, entry) in tmpl.entries.iter().enumerate() {
            let mut builder = NewEntry::builder();
            builder
                .id(EntryId::new())
                .transaction_id(transaction_id)
                .journal_id(journal_id)
                .sequence(zero_based_sequence as u32 + 1);
            let account_id: Uuid = entry.account_id.try_evaluate(ctx).map_err(|source| {
                PreparePostingRejection::Cel {
                    posting,
                    source: Box::new(source),
                }
            })?;
            builder.account_id(account_id);

            let entry_type: String = entry.entry_type.try_evaluate(ctx).map_err(|source| {
                PreparePostingRejection::Cel {
                    posting,
                    source: Box::new(source),
                }
            })?;
            builder.entry_type(entry_type);

            let layer: Layer =
                entry
                    .layer
                    .try_evaluate(ctx)
                    .map_err(|source| PreparePostingRejection::Cel {
                        posting,
                        source: Box::new(source),
                    })?;
            builder.layer(layer);

            let units: Decimal =
                entry
                    .units
                    .try_evaluate(ctx)
                    .map_err(|source| PreparePostingRejection::Cel {
                        posting,
                        source: Box::new(source),
                    })?;
            let currency: Currency = entry.currency.try_evaluate(ctx).map_err(|source| {
                PreparePostingRejection::Cel {
                    posting,
                    source: Box::new(source),
                }
            })?;
            let direction: DebitOrCredit = entry.direction.try_evaluate(ctx).map_err(|source| {
                PreparePostingRejection::Cel {
                    posting,
                    source: Box::new(source),
                }
            })?;

            let total = totals.entry((currency, layer)).or_insert(Decimal::ZERO);
            match direction {
                DebitOrCredit::Debit => *total -= units,
                DebitOrCredit::Credit => *total += units,
            };
            builder.units(units);
            builder.currency(currency);
            builder.direction(direction);

            if let Some(description) = entry.description.as_ref() {
                let description: String = description.try_evaluate(ctx).map_err(|source| {
                    PreparePostingRejection::Cel {
                        posting,
                        source: Box::new(source),
                    }
                })?;
                builder.description(description);
            }

            if let Some(metadata) = entry.metadata.as_ref() {
                let metadata: serde_json::Value =
                    metadata
                        .try_evaluate(ctx)
                        .map_err(|source| PreparePostingRejection::Cel {
                            posting,
                            source: Box::new(source),
                        })?;
                builder.metadata(metadata);
            }

            new_entries.push(builder.build().expect("Couldn't build entry"));
        }

        for ((c, l), v) in totals {
            if v != Decimal::ZERO {
                return Err(PreparePostingRejection::unbalanced(c, l, v, posting));
            }
        }

        Ok(new_entries)
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
