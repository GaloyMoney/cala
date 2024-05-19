use async_graphql::*;

use super::{convert::ToGlobalId, primitives::*};

#[derive(SimpleObject)]
pub(super) struct TxTemplate {
    pub id: ID,
    pub tx_template_id: UUID,
    pub code: String,
    pub params: Option<Vec<ParamDefinition>>,
    pub tx_input: TxInput,
    pub entries: Vec<EntryInput>,
    pub description: Option<String>,
    pub metadata: Option<JSON>,
}

#[derive(SimpleObject)]
pub(super) struct ParamDefinition {
    pub name: String,
    pub r#type: ParamDataType,
    pub default: Option<Expression>,
    pub description: Option<String>,
}

#[derive(SimpleObject)]
pub(super) struct EntryInput {
    pub entry_type: Expression,
    pub account_id: Expression,
    pub layer: Expression,
    pub direction: Expression,
    pub units: Expression,
    pub currency: Expression,
    pub description: Option<Expression>,
}

#[derive(SimpleObject)]
pub(super) struct TxInput {
    pub effective: Expression,
    pub journal_id: Expression,
    pub correlation_id: Option<Expression>,
    pub external_id: Option<Expression>,
    pub description: Option<Expression>,
    pub metadata: Option<Expression>,
}

#[derive(InputObject)]
pub(super) struct TxTemplateCreateInput {
    pub tx_template_id: UUID,
    pub code: String,
    pub params: Option<Vec<ParamDefinitionInput>>,
    pub tx_input: TxTemplateTxInput,
    pub entries: Vec<TxTemplateEntryInput>,
    pub description: Option<String>,
    pub metadata: Option<JSON>,
}

#[derive(InputObject)]
pub(super) struct TxTemplateTxInput {
    pub effective: Expression,
    pub journal_id: Expression,
    pub correlation_id: Option<Expression>,
    pub external_id: Option<Expression>,
    pub description: Option<Expression>,
    pub metadata: Option<Expression>,
}

#[derive(InputObject)]
pub(super) struct TxTemplateEntryInput {
    pub entry_type: Expression,
    pub account_id: Expression,
    pub layer: Expression,
    pub direction: Expression,
    pub units: Expression,
    pub currency: Expression,
    pub description: Option<Expression>,
}

#[derive(InputObject)]
pub(super) struct ParamDefinitionInput {
    pub name: String,
    pub r#type: ParamDataType,
    pub default: Option<Expression>,
    pub description: Option<String>,
}

#[derive(SimpleObject)]
pub(super) struct TxTemplateCreatePayload {
    pub tx_template: TxTemplate,
}

impl ToGlobalId for cala_ledger::TxTemplateId {
    fn to_global_id(&self) -> async_graphql::types::ID {
        async_graphql::types::ID::from(format!("tx_template:{}", self))
    }
}

impl From<cala_ledger::tx_template::TxTemplateValues> for TxTemplate {
    fn from(value: cala_ledger::tx_template::TxTemplateValues) -> Self {
        let tx_input = TxInput::from(value.tx_input);
        let entries = value.entries.into_iter().map(EntryInput::from).collect();
        let params = value
            .params
            .map(|params| params.into_iter().map(ParamDefinition::from).collect());
        Self {
            id: value.id.to_global_id(),
            tx_template_id: UUID::from(value.id),
            code: value.code,
            tx_input,
            entries,
            params,
            description: value.description,
            metadata: value.metadata.map(JSON::from),
        }
    }
}

impl From<cala_ledger::tx_template::TxInput> for TxInput {
    fn from(
        cala_ledger::tx_template::TxInput {
            effective,
            journal_id,
            correlation_id,
            external_id,
            description,
            metadata,
        }: cala_ledger::tx_template::TxInput,
    ) -> Self {
        Self {
            effective: Expression::from(effective),
            journal_id: Expression::from(journal_id),
            correlation_id: correlation_id.map(Expression::from),
            external_id: external_id.map(Expression::from),
            description: description.map(Expression::from),
            metadata: metadata.map(Expression::from),
        }
    }
}

impl From<cala_ledger::tx_template::EntryInput> for EntryInput {
    fn from(
        cala_ledger::tx_template::EntryInput {
            entry_type,
            account_id,
            layer,
            direction,
            units,
            currency,
            description,
        }: cala_ledger::tx_template::EntryInput,
    ) -> Self {
        Self {
            entry_type: Expression::from(entry_type),
            account_id: Expression::from(account_id),
            layer: Expression::from(layer),
            direction: Expression::from(direction),
            units: Expression::from(units),
            currency: Expression::from(currency),
            description: description.map(Expression::from),
        }
    }
}

impl From<cala_ledger::tx_template::ParamDefinition> for ParamDefinition {
    fn from(value: cala_ledger::tx_template::ParamDefinition) -> Self {
        let default = value.default.map(Expression::from);
        Self {
            name: value.name,
            r#type: ParamDataType::from(value.r#type),
            default,
            description: value.description,
        }
    }
}
