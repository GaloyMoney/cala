use async_graphql::*;

use super::primitives::*;

#[derive(SimpleObject)]
pub(super) struct TxTemplate {
    pub id: ID,
    pub tx_template_id: UUID,
    pub code: String,
    pub params: Vec<ParamDefinition>,
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

#[derive(Enum, Copy, Clone, PartialEq, Eq)]
#[graphql(remote = "cala_ledger::tx_template::ParamDataType")]
#[allow(clippy::upper_case_acronyms)]
pub enum ParamDataType {
    STRING,
    INTEGER,
    DECIMAL,
    BOOLEAN,
    UUID,
    DATE,
    TIMESTAMP,
    JSON,
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
    pub id: Option<UUID>,
    pub code: String,
    pub params: Vec<ParamDefinitionInput>,
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
