use chrono::{DateTime, NaiveDate, Utc};
use derive_builder::Builder;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use tracing::instrument;

pub use crate::param::definition::*;
use cala_types::primitives::{DebitOrCredit, Layer};
pub use cala_types::{primitives::TxTemplateId, tx_template::*};
use cel_interpreter::{CelError, CelExpression, CelMap, CelResult, CelValue, ResultCoercionError};
use es_entity::{clock::Clock, *};

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

#[derive(EsEntity, Builder)]
#[builder(pattern = "owned", build_fn(error = "EntityHydrationError"))]
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
        let mut builder = TxTemplateBuilder::default();
        for event in events.iter_all() {
            match event {
                TxTemplateEvent::Initialized { values } => {
                    builder = builder.id(values.id).values(values.clone());
                }
            }
        }
        builder.events(events).build()
    }
}

#[derive(Builder, Debug)]
#[builder(build_fn(validate = "Self::validate"))]
pub struct NewTxTemplate {
    #[builder(setter(into))]
    pub(super) id: TxTemplateId,
    #[builder(setter(into))]
    pub(super) code: String,
    #[builder(setter(strip_option, into), default)]
    pub(super) description: Option<String>,
    #[builder(setter(strip_option), default)]
    pub(super) params: Option<Vec<NewParamDefinition>>,
    pub(super) transaction: NewTxTemplateTransaction,
    pub(super) entries: Vec<NewTxTemplateEntry>,
    #[builder(setter(custom), default)]
    pub(super) metadata: Option<serde_json::Value>,
}

impl NewTxTemplate {
    pub fn builder() -> NewTxTemplateBuilder {
        NewTxTemplateBuilder::default()
    }
}

impl IntoEvents<TxTemplateEvent> for NewTxTemplate {
    fn into_events(self) -> EntityEvents<TxTemplateEvent> {
        EntityEvents::init(
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

impl NewTxTemplateBuilder {
    pub fn metadata<T: serde::Serialize>(
        &mut self,
        metadata: T,
    ) -> Result<&mut Self, serde_json::Error> {
        self.metadata = Some(Some(serde_json::to_value(metadata)?));
        Ok(self)
    }

    #[instrument(name = "tx_template.validate", skip(self), err(level = tracing::Level::WARN))]
    fn validate(&self) -> Result<(), String> {
        let mut ctx = crate::cel_context::initialize(Clock::handle().clone());
        let mut params_map = CelMap::new();
        if let Some(Some(defs)) = self.params.as_ref() {
            for def in defs {
                params_map.insert(def.name.clone(), dummy_value_for(&def.r#type));
            }
        }
        ctx.add_variable("params", params_map);

        if let Some(txn) = self.transaction.as_ref() {
            eval_expr(&ctx, &txn.effective, "transaction.effective")?;
            eval_expr(&ctx, &txn.journal_id, "transaction.journal_id")?;
            if let Some(s) = &txn.correlation_id {
                eval_expr(&ctx, s, "transaction.correlation_id")?;
            }
            if let Some(s) = &txn.external_id {
                eval_expr(&ctx, s, "transaction.external_id")?;
            }
            if let Some(s) = &txn.description {
                eval_expr(&ctx, s, "transaction.description")?;
            }
            if let Some(s) = &txn.metadata {
                eval_expr(&ctx, s, "transaction.metadata")?;
            }
        }

        if let Some(entries) = self.entries.as_ref() {
            for (i, e) in entries.iter().enumerate() {
                eval_expr(&ctx, &e.entry_type, &format!("entries[{i}].entry_type"))?;
                eval_expr(&ctx, &e.account_id, &format!("entries[{i}].account_id"))?;
                eval_expr(&ctx, &e.layer, &format!("entries[{i}].layer"))?;
                eval_expr(&ctx, &e.direction, &format!("entries[{i}].direction"))?;
                eval_expr(&ctx, &e.units, &format!("entries[{i}].units"))?;
                eval_expr(&ctx, &e.currency, &format!("entries[{i}].currency"))?;
                if let Some(s) = &e.description {
                    eval_expr(&ctx, s, &format!("entries[{i}].description"))?;
                }
                if let Some(s) = &e.metadata {
                    eval_expr(&ctx, s, &format!("entries[{i}].metadata"))?;
                }

                // Enum-literal check: only applies when the expression is a
                // bare quoted string. Dynamic refs like `params.dir` are
                // opaque here and get validated at post time.
                check_enum_literal::<Layer>(&e.layer, &format!("entries[{i}].layer"))?;
                check_enum_literal::<DebitOrCredit>(
                    &e.direction,
                    &format!("entries[{i}].direction"),
                )?;
            }
        }

        Ok(())
    }
}

fn dummy_value_for(t: &ParamDataType) -> CelValue {
    match t {
        ParamDataType::String => CelValue::from("stub"),
        ParamDataType::Integer => CelValue::from(1i64),
        ParamDataType::Decimal => CelValue::from(Decimal::ONE),
        ParamDataType::Boolean => CelValue::from(false),
        ParamDataType::Uuid => CelValue::from(uuid::Uuid::nil()),
        ParamDataType::Date => CelValue::from(NaiveDate::from_ymd_opt(2000, 1, 1).unwrap()),
        ParamDataType::Timestamp => CelValue::from(DateTime::<Utc>::from_timestamp(0, 0).unwrap()),
        ParamDataType::Json => CelValue::Map(std::sync::Arc::new(CelMap::new())),
    }
}

fn unknown_ident(err: &CelError) -> Option<&str> {
    match err {
        CelError::UnknownIdent(name) => Some(name.as_str()),
        CelError::EvaluationError(_, inner) => unknown_ident(inner),
        _ => None,
    }
}

fn eval_expr(ctx: &cel_interpreter::CelContext, expr: &str, field: &str) -> Result<(), String> {
    let compiled = CelExpression::try_from(expr).map_err(|e| format!("{field}: {e}"))?;
    match compiled.evaluate(ctx) {
        Ok(_) => Ok(()),
        Err(e) => match unknown_ident(&e) {
            Some(name) => Err(format!(
                "{field}: undeclared identifier '{name}' — declare it as a param or fix the reference"
            )),
            None => Ok(()),
        },
    }
}

/// Returns the inner value if `expr` is a bare single-quoted string literal.
///
/// Only handles the simple `'...'` shape used throughout the codebase for
/// enum literals (e.g. `'SETTLED'`, `'DEBIT'`). Anything else — expressions,
/// concatenations, escaped quotes — falls through and gets deferred to
/// runtime validation.
fn as_string_literal(expr: &str) -> Option<&str> {
    let trimmed = expr.trim();
    let inner = trimmed.strip_prefix('\'')?.strip_suffix('\'')?;
    if inner.contains(['\'', '\\']) {
        return None;
    }
    Some(inner)
}

fn check_enum_literal<T>(expr: &str, field: &str) -> Result<(), String>
where
    for<'a> T: TryFrom<CelResult<'a>, Error = ResultCoercionError>,
{
    let Some(literal) = as_string_literal(expr) else {
        return Ok(());
    };
    let val = CelValue::from(literal);
    let result = CelResult { expr, val };
    T::try_from(result)
        .map(|_| ())
        .map_err(|e| format!("{field}: {e}"))
}

#[derive(Clone, Debug, Builder)]
#[builder(build_fn(validate = "Self::validate"))]
pub struct NewTxTemplateEntry {
    #[builder(setter(into))]
    entry_type: String,
    #[builder(setter(into))]
    account_id: String,
    #[builder(setter(into))]
    layer: String,
    #[builder(setter(into))]
    direction: String,
    #[builder(setter(into))]
    units: String,
    #[builder(setter(into))]
    currency: String,
    #[builder(setter(strip_option, into), default)]
    description: Option<String>,
    #[builder(setter(strip_option, into), default)]
    metadata: Option<String>,
}

impl NewTxTemplateEntry {
    pub fn builder() -> NewTxTemplateEntryBuilder {
        NewTxTemplateEntryBuilder::default()
    }
}
impl NewTxTemplateEntryBuilder {
    #[instrument(name = "tx_template_entry.validate", skip(self), err(level = tracing::Level::WARN))]
    fn validate(&self) -> Result<(), String> {
        validate_expression(
            self.entry_type
                .as_ref()
                .expect("Mandatory field 'entry_type' not set"),
        )?;
        validate_expression(
            self.account_id
                .as_ref()
                .expect("Mandatory field 'account_id' not set"),
        )?;
        validate_expression(
            self.layer
                .as_ref()
                .expect("Mandatory field 'layer' not set"),
        )?;
        validate_expression(
            self.direction
                .as_ref()
                .expect("Mandatory field 'direction' not set"),
        )?;
        validate_expression(
            self.units
                .as_ref()
                .expect("Mandatory field 'units' not set"),
        )?;
        validate_expression(
            self.currency
                .as_ref()
                .expect("Mandatory field 'currency' not set"),
        )?;
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
#[derive(Clone, Debug, Serialize, Builder, Deserialize)]
#[builder(build_fn(validate = "Self::validate"))]
pub struct NewTxTemplateTransaction {
    #[builder(setter(into))]
    effective: String,
    #[builder(setter(into))]
    journal_id: String,
    #[builder(setter(strip_option, into), default)]
    correlation_id: Option<String>,
    #[builder(setter(strip_option, into), default)]
    external_id: Option<String>,
    #[builder(setter(strip_option, into), default)]
    description: Option<String>,
    #[builder(setter(strip_option, into), default)]
    metadata: Option<String>,
}

impl NewTxTemplateTransaction {
    pub fn builder() -> NewTxTemplateTransactionBuilder {
        NewTxTemplateTransactionBuilder::default()
    }
}

impl NewTxTemplateTransactionBuilder {
    #[instrument(name = "tx_template_transaction.validate", skip(self), err(level = tracing::Level::WARN))]
    fn validate(&self) -> Result<(), String> {
        validate_expression(
            self.effective
                .as_ref()
                .expect("Mandatory field 'effective' not set"),
        )?;
        validate_expression(
            self.journal_id
                .as_ref()
                .expect("Mandatory field 'journal_id' not set"),
        )?;
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
fn validate_expression(expr: &str) -> Result<(), String> {
    CelExpression::try_from(expr).map_err(|e| e.to_string())?;
    Ok(())
}

#[instrument(name = "tx_template.validate_optional_expression", skip(expr), err(level = tracing::Level::WARN))]
fn validate_optional_expression(expr: &Option<Option<String>>) -> Result<(), String> {
    if let Some(Some(expr)) = expr.as_ref() {
        CelExpression::try_from(expr.as_str()).map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn valid_entry() -> NewTxTemplateEntry {
        NewTxTemplateEntry::builder()
            .entry_type("'TEST_DR'")
            .account_id("params.recipient")
            .layer("'SETTLED'")
            .direction("'DEBIT'")
            .units("1290")
            .currency("'BTC'")
            .metadata(r#"{"sender": params.sender}"#)
            .build()
            .unwrap()
    }

    fn valid_params() -> Vec<NewParamDefinition> {
        vec![
            NewParamDefinition::builder()
                .name("recipient")
                .r#type(ParamDataType::Uuid)
                .build()
                .unwrap(),
            NewParamDefinition::builder()
                .name("sender")
                .r#type(ParamDataType::Uuid)
                .build()
                .unwrap(),
        ]
    }

    fn template_with(entries: Vec<NewTxTemplateEntry>) -> Result<NewTxTemplate, String> {
        let journal_id = Uuid::now_v7();
        NewTxTemplate::builder()
            .id(TxTemplateId::new())
            .code("CODE")
            .params(valid_params())
            .transaction(
                NewTxTemplateTransaction::builder()
                    .effective("date('2022-11-01')")
                    .journal_id(format!("uuid('{journal_id}')"))
                    .build()
                    .unwrap(),
            )
            .entries(entries)
            .build()
            .map_err(|e| e.to_string())
    }

    #[test]
    fn it_builds() {
        let new_tx_template = template_with(vec![valid_entry()]).unwrap();
        assert_eq!(new_tx_template.description, None);
    }

    #[test]
    fn fails_when_mandatory_fields_are_missing() {
        let new_tx_template = NewTxTemplate::builder().build();
        assert!(new_tx_template.is_err());
    }

    #[test]
    fn rejects_singular_param_reference() {
        let entry = NewTxTemplateEntry::builder()
            .entry_type("'X'")
            .account_id("param.recipient") // singular: never populated in the CEL context
            .layer("'SETTLED'")
            .direction("'DEBIT'")
            .units("1")
            .currency("'BTC'")
            .build()
            .unwrap();
        let err = template_with(vec![entry]).unwrap_err();
        assert!(err.contains("account_id"), "err was: {err}");
        assert!(err.contains("param"), "err was: {err}");
    }

    #[test]
    fn rejects_undeclared_param_key() {
        let entry = NewTxTemplateEntry::builder()
            .entry_type("'X'")
            .account_id("params.not_declared") // plural but never declared
            .layer("'SETTLED'")
            .direction("'DEBIT'")
            .units("1")
            .currency("'BTC'")
            .build()
            .unwrap();
        let err = template_with(vec![entry]).unwrap_err();
        assert!(err.contains("not_declared"), "err was: {err}");
    }

    #[test]
    fn rejects_params_reference_when_no_params_declared() {
        let entry = NewTxTemplateEntry::builder()
            .entry_type("'X'")
            .account_id("params.whatever")
            .layer("'SETTLED'")
            .direction("'DEBIT'")
            .units("1")
            .currency("'BTC'")
            .build()
            .unwrap();
        let journal_id = Uuid::now_v7();
        let err = NewTxTemplate::builder()
            .id(TxTemplateId::new())
            .code("CODE")
            // no `.params(...)` call — nothing declared
            .transaction(
                NewTxTemplateTransaction::builder()
                    .effective("date('2022-11-01')")
                    .journal_id(format!("uuid('{journal_id}')"))
                    .build()
                    .unwrap(),
            )
            .entries(vec![entry])
            .build()
            .unwrap_err()
            .to_string();
        assert!(err.contains("whatever"), "err was: {err}");
    }

    #[test]
    fn rejects_invalid_layer_literal() {
        let entry = NewTxTemplateEntry::builder()
            .entry_type("'X'")
            .account_id("params.recipient")
            .layer("'Settled'") // wrong case: Layer::try_from expects uppercase
            .direction("'DEBIT'")
            .units("1")
            .currency("'BTC'")
            .build()
            .unwrap();
        let err = template_with(vec![entry]).unwrap_err();
        assert!(err.contains("layer"), "err was: {err}");
    }

    #[test]
    fn rejects_invalid_direction_literal() {
        let entry = NewTxTemplateEntry::builder()
            .entry_type("'X'")
            .account_id("params.recipient")
            .layer("'SETTLED'")
            .direction("'Settled'") // not a valid DebitOrCredit
            .units("1")
            .currency("'BTC'")
            .build()
            .unwrap();
        let err = template_with(vec![entry]).unwrap_err();
        assert!(err.contains("direction"), "err was: {err}");
    }

    #[test]
    fn accepts_dynamic_direction_and_layer_from_declared_param() {
        // When direction/layer are param references (not literals), we can't
        // check the runtime value at build time — validation must not reject.
        let params = vec![
            NewParamDefinition::builder()
                .name("recipient")
                .r#type(ParamDataType::Uuid)
                .build()
                .unwrap(),
            NewParamDefinition::builder()
                .name("sender")
                .r#type(ParamDataType::Uuid)
                .build()
                .unwrap(),
            NewParamDefinition::builder()
                .name("dir")
                .r#type(ParamDataType::String)
                .build()
                .unwrap(),
            NewParamDefinition::builder()
                .name("lyr")
                .r#type(ParamDataType::String)
                .build()
                .unwrap(),
        ];
        let entry = NewTxTemplateEntry::builder()
            .entry_type("'X'")
            .account_id("params.recipient")
            .layer("params.lyr")
            .direction("params.dir")
            .units("1")
            .currency("'BTC'")
            .build()
            .unwrap();
        let journal_id = Uuid::now_v7();
        let result = NewTxTemplate::builder()
            .id(TxTemplateId::new())
            .code("CODE")
            .params(params)
            .transaction(
                NewTxTemplateTransaction::builder()
                    .effective("date('2022-11-01')")
                    .journal_id(format!("uuid('{journal_id}')"))
                    .build()
                    .unwrap(),
            )
            .entries(vec![entry])
            .build();
        assert!(
            result.is_ok(),
            "template with dynamic enum refs was rejected"
        );
    }
}
