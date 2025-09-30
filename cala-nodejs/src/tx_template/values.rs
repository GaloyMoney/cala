#[napi(object)]
pub struct NewParamDefinitionValues {
  pub name: String,
  pub r#type: ParamDataTypeValues,
  pub default: Option<String>,
  pub description: Option<String>,
}

#[napi(object)]
pub struct NewTxTemplateEntryValues {
  pub entry_type: String,
  pub account_id: String,
  pub layer: String,
  pub direction: String,
  pub units: String,
  pub currency: String,
  pub description: Option<String>,
  pub metadata: Option<String>,
}

#[napi(object)]
pub struct NewTxTemplateValues {
  pub id: Option<String>,
  pub code: String,
  pub external_id: Option<String>,
  pub description: Option<String>,
  pub params: Option<Vec<NewParamDefinitionValues>>,
  pub entries: Vec<NewTxTemplateEntryValues>,
  pub metadata: Option<serde_json::Value>,
  pub transaction: Option<NewTxTemplateTransactionValues>,
}

#[napi(object)]
pub struct NewTxTemplateTransactionValues {
  pub effective: String,
  pub journal_id: String,
  pub correlation_id: Option<String>,
  pub external_id: Option<String>,
  pub description: Option<String>,
  pub metadata: Option<String>,
}

#[napi(object)]
pub struct TxTemplateValues {
  pub id: String,
  pub code: String,
  pub version: u32,
  pub metadata: Option<serde_json::Value>,
  pub description: Option<String>,
}

#[napi]
pub enum ParamDataTypeValues {
  String,
  Integer,
  Decimal,
  Boolean,
  Uuid,
  Date,
  Timestamp,
  Json,
}

impl From<&cala_ledger::tx_template::TxTemplate> for TxTemplateValues {
  fn from(template: &cala_ledger::tx_template::TxTemplate) -> Self {
    let values = template.values().clone();
    Self {
      id: values.id.to_string(),
      code: values.code.to_string(),
      description: values.description,
      metadata: values.metadata,
      version: values.version,
    }
  }
}

impl From<cala_ledger::tx_template::TxTemplate> for TxTemplateValues {
  fn from(template: cala_ledger::tx_template::TxTemplate) -> Self {
    let values = template.into_values();
    Self {
      id: values.id.to_string(),
      code: values.code.to_string(),
      description: values.description,
      metadata: values.metadata,
      version: values.version,
    }
  }
}
