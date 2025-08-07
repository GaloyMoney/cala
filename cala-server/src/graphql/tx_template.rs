use async_graphql::*;

use super::{convert::ToGlobalId, primitives::*};

#[derive(Clone, SimpleObject)]
pub struct TxTemplate {
    id: ID,
    tx_template_id: UUID,
    version: u32,
    code: String,
    params: Option<Vec<ParamDefinition>>,
    transaction: TxTemplateTransaction,
    entries: Vec<TxTemplateEntry>,
    description: Option<String>,
    metadata: Option<JSON>,
    created_at: Timestamp,
    modified_at: Timestamp,
}

#[derive(Clone, SimpleObject)]
pub(super) struct ParamDefinition {
    name: String,
    r#type: ParamDataType,
    default: Option<Expression>,
    description: Option<String>,
}

#[derive(Clone, SimpleObject)]
pub(super) struct TxTemplateEntry {
    entry_type: Expression,
    account_id: Expression,
    layer: Expression,
    direction: Expression,
    units: Expression,
    currency: Expression,
    description: Option<Expression>,
    metadata: Option<Expression>,
}

#[derive(Clone, SimpleObject)]
pub(super) struct TxTemplateTransaction {
    effective: Expression,
    journal_id: Expression,
    correlation_id: Option<Expression>,
    external_id: Option<Expression>,
    description: Option<Expression>,
    metadata: Option<Expression>,
}

#[derive(InputObject)]
pub(super) struct TxTemplateCreateInput {
    pub tx_template_id: UUID,
    pub code: String,
    pub params: Option<Vec<ParamDefinitionInput>>,
    pub transaction: TxTemplateTransactionInput,
    pub entries: Vec<TxTemplateEntryInput>,
    pub description: Option<String>,
    pub metadata: Option<JSON>,
}

#[derive(InputObject)]
pub(super) struct TxTemplateTransactionInput {
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
        async_graphql::types::ID::from(format!("tx_template:{self}"))
    }
}

impl From<cala_ledger::tx_template::TxTemplate> for TxTemplate {
    fn from(entity: cala_ledger::tx_template::TxTemplate) -> Self {
        let created_at = entity.created_at();
        let modified_at = entity.modified_at();
        let values = entity.into_values();
        let transaction = TxTemplateTransaction::from(values.transaction);
        let entries = values
            .entries
            .into_iter()
            .map(TxTemplateEntry::from)
            .collect();
        let params = values
            .params
            .map(|params| params.into_iter().map(ParamDefinition::from).collect());
        Self {
            id: values.id.to_global_id(),
            version: values.version,
            tx_template_id: UUID::from(values.id),
            code: values.code,
            transaction,
            entries,
            params,
            description: values.description,
            metadata: values.metadata.map(JSON::from),
            created_at: Timestamp::from(created_at),
            modified_at: Timestamp::from(modified_at),
        }
    }
}

impl From<cala_ledger::tx_template::TxTemplateTransaction> for TxTemplateTransaction {
    fn from(
        cala_ledger::tx_template::TxTemplateTransaction {
            effective,
            journal_id,
            correlation_id,
            external_id,
            description,
            metadata,
        }: cala_ledger::tx_template::TxTemplateTransaction,
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

impl From<cala_ledger::tx_template::TxTemplateEntry> for TxTemplateEntry {
    fn from(
        cala_ledger::tx_template::TxTemplateEntry {
            entry_type,
            account_id,
            layer,
            direction,
            units,
            currency,
            description,
            metadata,
        }: cala_ledger::tx_template::TxTemplateEntry,
    ) -> Self {
        Self {
            entry_type: Expression::from(entry_type),
            account_id: Expression::from(account_id),
            layer: Expression::from(layer),
            direction: Expression::from(direction),
            units: Expression::from(units),
            currency: Expression::from(currency),
            description: description.map(Expression::from),
            metadata: metadata.map(Expression::from),
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

impl From<cala_ledger::tx_template::TxTemplate> for TxTemplateCreatePayload {
    fn from(entity: cala_ledger::tx_template::TxTemplate) -> Self {
        Self {
            tx_template: TxTemplate::from(entity),
        }
    }
}
