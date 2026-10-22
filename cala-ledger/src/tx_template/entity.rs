use serde::{Deserialize, Serialize};
use tracing::instrument;

pub use crate::param::definition::*;
pub use cala_types::{primitives::TxTemplateId, tx_template::*};
use cel_interpreter::{CelExpression, CelParseRejection};
use es_entity::*;

#[derive(EsEvent, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
#[serde(tag = "type", rename_all = "snake_case")]
#[es_event(id = "TxTemplateId", event_context = false)]
pub enum TxTemplateEvent {
    Initialized { values: TxTemplateValues },
}

impl TxTemplateEvent {
    pub fn into_values(self) -> TxTemplateValues {
        match self {
            TxTemplateEvent::Initialized { values } => values,
        }
    }
}

#[derive(EsEntity)]
pub struct TxTemplate {
    pub id: TxTemplateId,
    values: TxTemplateValues,
    events: EntityEvents<TxTemplateEvent>,
}

impl TxTemplate {
    pub fn id(&self) -> TxTemplateId {
        self.values.id
    }

    pub fn values(&self) -> &TxTemplateValues {
        &self.values
    }

    pub fn into_values(self) -> TxTemplateValues {
        self.values
    }

    pub fn created_at(&self) -> chrono::DateTime<chrono::Utc> {
        self.events
            .entity_first_persisted_at()
            .expect("No persisted events")
    }

    pub fn modified_at(&self) -> chrono::DateTime<chrono::Utc> {
        self.events
            .entity_last_modified_at()
            .expect("No events for account")
    }
}

impl TryFromEvents<TxTemplateEvent> for TxTemplate {
    fn try_from_events(
        events: EntityEvents<TxTemplateEvent>,
    ) -> Result<Self, EntityHydrationError> {
        let mut hydrator = TxTemplate::hydrator();
        for event in events.iter_all() {
            match event {
                TxTemplateEvent::Initialized { values } => {
                    hydrator.id(values.id).values(values.clone());
                }
            }
        }
        hydrator.build(events)
    }
}

#[derive(bon::Builder, Debug)]
pub struct NewTxTemplate {
    #[builder(into)]
    pub(super) id: TxTemplateId,
    #[builder(into)]
    pub(super) code: String,
    #[builder(into)]
    pub(super) description: Option<String>,
    pub(super) params: Option<Vec<NewParamDefinition>>,
    pub(super) transaction: NewTxTemplateTransaction,
    pub(super) entries: Vec<NewTxTemplateEntry>,
    #[builder(with = |metadata: impl serde::Serialize| -> Result<_, serde_json::Error> {
        serde_json::to_value(metadata)
    })]
    pub(super) metadata: Option<serde_json::Value>,
}

impl IntoEvents<TxTemplateEvent> for NewTxTemplate {
    type IdSource = FromCaller;

    fn into_events(
        self,
        builder: EntityEventsBuilder<TxTemplateEvent, FromCaller>,
    ) -> EntityEvents<TxTemplateEvent> {
        builder.initial_events(
            self.id,
            [TxTemplateEvent::Initialized {
                values: TxTemplateValues {
                    id: self.id,
                    version: 1,
                    code: self.code,
                    description: self.description,
                    params: self
                        .params
                        .map(|p| p.into_iter().map(|p| p.into()).collect()),
                    transaction: self.transaction.into(),
                    entries: self.entries.into_iter().map(|e| e.into()).collect(),
                    metadata: self.metadata,
                },
            }],
        )
    }
}

#[derive(Clone, Debug)]
pub struct NewTxTemplateEntry {
    entry_type: String,
    account_id: String,
    layer: String,
    direction: String,
    units: String,
    currency: String,
    description: Option<String>,
    metadata: Option<String>,
}

#[bon::bon]
impl NewTxTemplateEntry {
    #[builder]
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        #[builder(into)] entry_type: String,
        #[builder(into)] account_id: String,
        #[builder(into)] layer: String,
        #[builder(into)] direction: String,
        #[builder(into)] units: String,
        #[builder(into)] currency: String,
        #[builder(into)] description: Option<String>,
        #[builder(into)] metadata: Option<String>,
    ) -> Result<Self, CelParseRejection> {
        let entry = Self {
            entry_type,
            account_id,
            layer,
            direction,
            units,
            currency,
            description,
            metadata,
        };
        entry.validate()?;
        Ok(entry)
    }

    #[instrument(name = "tx_template_entry.validate", skip(self), err(level = tracing::Level::WARN))]
    fn validate(&self) -> Result<(), CelParseRejection> {
        validate_expression(&self.entry_type)?;
        validate_expression(&self.account_id)?;
        validate_expression(&self.layer)?;
        validate_expression(&self.direction)?;
        validate_expression(&self.units)?;
        validate_expression(&self.currency)?;
        validate_optional_expression(&self.description)?;
        validate_optional_expression(&self.metadata)
    }
}

impl From<NewTxTemplateEntry> for cala_types::tx_template::TxTemplateEntry {
    fn from(input: NewTxTemplateEntry) -> Self {
        cala_types::tx_template::TxTemplateEntry {
            entry_type: CelExpression::try_from(input.entry_type)
                .expect("always a valid entry type"),
            account_id: CelExpression::try_from(input.account_id)
                .expect("always a valid account id"),
            layer: CelExpression::try_from(input.layer).expect("always a valid layer"),
            direction: CelExpression::try_from(input.direction).expect("always a valid direction"),
            units: CelExpression::try_from(input.units).expect("always a valid units"),
            currency: CelExpression::try_from(input.currency).expect("always a valid currency"),
            description: input
                .description
                .map(|d| CelExpression::try_from(d).expect("always a valid description")),
            metadata: input
                .metadata
                .map(|m| CelExpression::try_from(m).expect("always a valid metadata")),
        }
    }
}

/// Contains the transaction-level details needed to create a `Transaction`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NewTxTemplateTransaction {
    effective: String,
    journal_id: String,
    correlation_id: Option<String>,
    external_id: Option<String>,
    description: Option<String>,
    metadata: Option<String>,
}

#[bon::bon]
impl NewTxTemplateTransaction {
    #[builder]
    pub fn new(
        #[builder(into)] effective: String,
        #[builder(into)] journal_id: String,
        #[builder(into)] correlation_id: Option<String>,
        #[builder(into)] external_id: Option<String>,
        #[builder(into)] description: Option<String>,
        #[builder(into)] metadata: Option<String>,
    ) -> Result<Self, CelParseRejection> {
        let transaction = Self {
            effective,
            journal_id,
            correlation_id,
            external_id,
            description,
            metadata,
        };
        transaction.validate()?;
        Ok(transaction)
    }

    #[instrument(name = "tx_template_transaction.validate", skip(self), err(level = tracing::Level::WARN))]
    fn validate(&self) -> Result<(), CelParseRejection> {
        validate_expression(&self.effective)?;
        validate_expression(&self.journal_id)?;
        validate_optional_expression(&self.correlation_id)?;
        validate_optional_expression(&self.external_id)?;
        validate_optional_expression(&self.description)?;
        validate_optional_expression(&self.metadata)
    }
}

impl From<NewTxTemplateTransaction> for cala_types::tx_template::TxTemplateTransaction {
    fn from(
        NewTxTemplateTransaction {
            effective,
            journal_id,
            correlation_id,
            external_id,
            description,
            metadata,
        }: NewTxTemplateTransaction,
    ) -> Self {
        cala_types::tx_template::TxTemplateTransaction {
            effective: CelExpression::try_from(effective).expect("always a valid effective date"),
            journal_id: CelExpression::try_from(journal_id).expect("always a valid journal id"),
            correlation_id: correlation_id
                .map(|c| CelExpression::try_from(c).expect("always a valid correlation id")),
            external_id: external_id
                .map(|id| CelExpression::try_from(id).expect("always a valid external id")),
            description: description
                .map(|d| CelExpression::try_from(d).expect("always a valid description")),
            metadata: metadata
                .map(|m| CelExpression::try_from(m).expect("always a valid metadata")),
        }
    }
}

#[instrument(name = "tx_template.validate_expression", skip(expr), fields(expression = %expr), err(level = tracing::Level::WARN))]
fn validate_expression(expr: &str) -> Result<(), CelParseRejection> {
    CelExpression::try_from(expr)?;
    Ok(())
}

#[instrument(name = "tx_template.validate_optional_expression", skip(expr), err(level = tracing::Level::WARN))]
fn validate_optional_expression(expr: &Option<String>) -> Result<(), CelParseRejection> {
    if let Some(expr) = expr {
        CelExpression::try_from(expr.as_str())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn it_builds() {
        let journal_id = Uuid::now_v7();
        let entries = vec![NewTxTemplateEntry::builder()
            .entry_type("'TEST_DR'")
            .account_id("param.recipient")
            .layer("'Settled'")
            .direction("'Settled'")
            .units("1290")
            .currency("'BTC'")
            .metadata(r#"{"sender": param.sender}"#)
            .build()
            .unwrap()];
        let new_tx_template = NewTxTemplate::builder()
            .id(TxTemplateId::new())
            .code("CODE")
            .transaction(
                NewTxTemplateTransaction::builder()
                    .effective("date('2022-11-01')")
                    .journal_id(format!("uuid('{journal_id}')"))
                    .build()
                    .unwrap(),
            )
            .entries(entries)
            .build();
        assert_eq!(new_tx_template.description, None);
    }

    #[test]
    fn rejects_invalid_expressions() {
        let entry = NewTxTemplateEntry::builder()
            .entry_type("'TEST_DR'")
            .account_id("param.recipient")
            .layer("'Settled'")
            .direction("'Settled'")
            .units("1290 +")
            .currency("'BTC'")
            .build();
        assert!(matches!(entry, Err(CelParseRejection::ParseError(_))));

        let transaction = NewTxTemplateTransaction::builder()
            .effective("date('2022-11-01')")
            .journal_id("uuid(")
            .build();
        assert!(matches!(transaction, Err(CelParseRejection::ParseError(_))));
    }
}
