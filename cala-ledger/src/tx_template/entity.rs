use derive_builder::Builder;
use serde::{Deserialize, Serialize};

pub use cala_types::{primitives::TxTemplateId, tx_template::*};
use cel_interpreter::CelExpression;

use crate::entity::*;
#[cfg(feature = "import")]
use crate::primitives::*;

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TxTemplateEvent {
    #[cfg(feature = "import")]
    Imported {
        source: DataSource,
        values: TxTemplateValues,
    },
    Initialized {
        values: TxTemplateValues,
    },
}

impl EntityEvent for TxTemplateEvent {
    type EntityId = TxTemplateId;
    fn event_table_name() -> &'static str {
        "cala_tx_template_events"
    }
}

#[derive(Builder)]
#[builder(pattern = "owned", build_fn(error = "EntityError"))]
pub struct TxTemplate {
    values: TxTemplateValues,
    pub(super) events: EntityEvents<TxTemplateEvent>,
}

impl Entity for TxTemplate {
    type Event = TxTemplateEvent;
}

impl TxTemplate {
    #[cfg(feature = "import")]
    pub(super) fn import(source: DataSourceId, values: TxTemplateValues) -> Self {
        let events = EntityEvents::init(
            values.id,
            [TxTemplateEvent::Imported {
                source: DataSource::Remote { id: source },
                values,
            }],
        );
        Self::try_from(events).expect("Failed to build tx_template from events")
    }

    pub fn id(&self) -> TxTemplateId {
        self.values.id
    }

    pub fn values(&self) -> &TxTemplateValues {
        &self.values
    }

    pub fn into_values(self) -> TxTemplateValues {
        self.values
    }
}

impl TryFrom<EntityEvents<TxTemplateEvent>> for TxTemplate {
    type Error = EntityError;

    fn try_from(events: EntityEvents<TxTemplateEvent>) -> Result<Self, Self::Error> {
        let mut builder = TxTemplateBuilder::default();
        for event in events.iter() {
            match event {
                #[cfg(feature = "import")]
                TxTemplateEvent::Imported { source: _, values } => {
                    builder = builder.values(values.clone());
                }
                TxTemplateEvent::Initialized { values } => {
                    builder = builder.values(values.clone());
                }
            }
        }
        builder.events(events).build()
    }
}

#[derive(Builder, Debug)]
pub struct NewTxTemplate {
    #[builder(setter(into))]
    pub(super) id: TxTemplateId,
    #[builder(setter(into))]
    pub(super) code: String,
    #[builder(setter(strip_option, into), default)]
    pub(super) description: Option<String>,
    #[builder(setter(strip_option), default)]
    pub(super) params: Option<Vec<NewParamDefinition>>,
    pub(super) tx_input: NewTxInput,
    pub(super) entries: Vec<NewEntryInput>,
    #[builder(setter(custom), default)]
    pub(super) metadata: Option<serde_json::Value>,
}

impl NewTxTemplate {
    pub fn builder() -> NewTxTemplateBuilder {
        NewTxTemplateBuilder::default()
    }

    pub(super) fn initial_events(self) -> EntityEvents<TxTemplateEvent> {
        EntityEvents::init(
            self.id,
            [TxTemplateEvent::Initialized {
                values: TxTemplateValues {
                    id: self.id,
                    code: self.code,
                    description: self.description,
                    params: self
                        .params
                        .map(|p| p.into_iter().map(|p| p.into()).collect()),
                    tx_input: self.tx_input.into(),
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
}

#[derive(Clone, Debug, Serialize, Builder, Deserialize)]
#[builder(build_fn(validate = "Self::validate"))]
pub struct NewEntryInput {
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
}

impl NewEntryInput {
    pub fn builder() -> NewEntryInputBuilder {
        NewEntryInputBuilder::default()
    }
}
impl NewEntryInputBuilder {
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
        validate_optional_expression(&self.description)
    }
}

impl From<NewEntryInput> for cala_types::tx_template::EntryInput {
    fn from(input: NewEntryInput) -> Self {
        cala_types::tx_template::EntryInput {
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
        }
    }
}

/// Contains the transaction-level details needed to create a `Transaction`.
#[derive(Clone, Debug, Serialize, Builder, Deserialize)]
#[builder(build_fn(validate = "Self::validate"))]
pub struct NewTxInput {
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

impl NewTxInput {
    pub fn builder() -> NewTxInputBuilder {
        NewTxInputBuilder::default()
    }
}

impl NewTxInputBuilder {
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

impl From<NewTxInput> for cala_types::tx_template::TxInput {
    fn from(
        NewTxInput {
            effective,
            journal_id,
            correlation_id,
            external_id,
            description,
            metadata,
        }: NewTxInput,
    ) -> Self {
        cala_types::tx_template::TxInput {
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

fn validate_expression(expr: &str) -> Result<(), String> {
    CelExpression::try_from(expr).map_err(|e| e.to_string())?;
    Ok(())
}
fn validate_optional_expression(expr: &Option<Option<String>>) -> Result<(), String> {
    if let Some(Some(expr)) = expr.as_ref() {
        CelExpression::try_from(expr.as_str()).map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[derive(Clone, Debug, Deserialize, Serialize, Builder)]
#[builder(build_fn(validate = "Self::validate"))]
pub struct NewParamDefinition {
    #[builder(setter(into))]
    pub(super) name: String,
    pub(super) r#type: ParamDataType,
    #[builder(setter(strip_option, name = "default_expr", into), default)]
    pub(super) default: Option<String>,
    #[builder(setter(strip_option, into), default)]
    pub(super) description: Option<String>,
}

impl NewParamDefinition {
    pub fn builder() -> NewParamDefinitionBuilder {
        NewParamDefinitionBuilder::default()
    }

    pub fn default_expr(&self) -> Option<CelExpression> {
        self.default
            .as_ref()
            .map(|v| v.parse().expect("Couldn't create default_expr"))
    }
}

impl NewParamDefinitionBuilder {
    fn validate(&self) -> Result<(), String> {
        if let Some(Some(expr)) = self.default.as_ref() {
            let expr = CelExpression::try_from(expr.as_str()).map_err(|e| e.to_string())?;
            let param_type = ParamDataType::try_from(
                &expr
                    .evaluate(&super::cel_context::initialize())
                    .map_err(|e| format!("{e}"))?,
            )?;
            let specified_type = self.r#type.as_ref().unwrap();
            if &param_type != specified_type {
                return Err(format!(
                    "Default expression type {param_type:?} does not match parameter type {specified_type:?}"
                ));
            }
        }
        Ok(())
    }
}

impl From<NewParamDefinition> for cala_types::tx_template::ParamDefinition {
    fn from(param: NewParamDefinition) -> Self {
        let default = param.default_expr();
        cala_types::tx_template::ParamDefinition {
            name: param.name,
            r#type: param.r#type,
            default,
            description: param.description,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn it_builds() {
        let journal_id = Uuid::new_v4();
        let entries = vec![NewEntryInput::builder()
            .entry_type("'TEST_DR'")
            .account_id("param.recipient")
            .layer("'Settled'")
            .direction("'Settled'")
            .units("1290")
            .currency("'BTC'")
            .build()
            .unwrap()];
        let new_tx_template = NewTxTemplate::builder()
            .id(TxTemplateId::new())
            .code("CODE")
            .tx_input(
                NewTxInput::builder()
                    .effective("date('2022-11-01')")
                    .journal_id(format!("uuid('{journal_id}')"))
                    .build()
                    .unwrap(),
            )
            .entries(entries)
            .build()
            .unwrap();
        assert_eq!(new_tx_template.description, None);
    }

    #[test]
    fn fails_when_mandatory_fields_are_missing() {
        let new_tx_template = NewTxTemplate::builder().build();
        assert!(new_tx_template.is_err());
    }

    #[test]
    fn build_param_definition() {
        let definition = NewParamDefinition::builder()
            .name("name")
            .r#type(ParamDataType::Json)
            .default_expr("{'key': 'value'}")
            .build()
            .unwrap();
        assert_eq!(definition.name, "name");
    }
}
