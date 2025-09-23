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
