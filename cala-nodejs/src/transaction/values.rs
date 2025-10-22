#[napi(object)]
pub struct TransactionValues {
  pub id: String,
  pub version: String,
  pub created_at: String,
  pub modified_at: String,
  pub journal_id: String,
  pub tx_template_id: String,
  pub entry_ids: Vec<String>,
  pub effective: String,
  pub correlation_id: String,
  pub external_id: Option<String>,
  pub description: Option<String>,
  pub void_of: Option<String>,
  pub voided_by: Option<String>,
  pub metadata: Option<serde_json::Value>,
}

impl From<&cala_ledger::transaction::Transaction> for TransactionValues {
  fn from(transaction: &cala_ledger::transaction::Transaction) -> Self {
    let values = transaction.values().clone();
    let effective = values.effective.format("%Y-%m-%d").to_string();
    Self {
      id: values.id.to_string(),
      version: values.version.to_string(),
      created_at: values.created_at.to_rfc3339(),
      modified_at: values.modified_at.to_rfc3339(),
      journal_id: values.journal_id.to_string(),
      tx_template_id: values.tx_template_id.to_string(),
      entry_ids: values.entry_ids.iter().map(|id| id.to_string()).collect(),
      effective,
      correlation_id: values.correlation_id.to_string(),
      external_id: values.external_id,
      description: values.description,
      void_of: values.void_of.map(|id| id.to_string()),
      voided_by: values.voided_by.map(|id| id.to_string()),
      metadata: values.metadata,
    }
  }
}

impl From<cala_ledger::transaction::Transaction> for TransactionValues {
  fn from(transaction: cala_ledger::transaction::Transaction) -> Self {
    let values = transaction.values().clone();
    let effective = values.effective.format("%Y-%m-%d").to_string();
    Self {
      id: values.id.to_string(),
      version: values.version.to_string(),
      created_at: values.created_at.to_rfc3339(),
      modified_at: values.modified_at.to_rfc3339(),
      journal_id: values.journal_id.to_string(),
      tx_template_id: values.tx_template_id.to_string(),
      entry_ids: values.entry_ids.iter().map(|id| id.to_string()).collect(),
      effective,
      correlation_id: values.correlation_id.to_string(),
      external_id: values.external_id,
      description: values.description,
      void_of: values.void_of.map(|id| id.to_string()),
      voided_by: values.voided_by.map(|id| id.to_string()),
      metadata: values.metadata,
    }
  }
}
