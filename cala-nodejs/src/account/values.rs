#[napi(object)]
pub struct AccountValues {
  pub id: String,
  pub code: String,
  pub name: String,
  pub tags: Vec<String>,
  pub external_id: Option<String>,
  pub description: Option<String>,
  pub metadata: Option<serde_json::Value>,
}

impl From<&cala_ledger::account::Account> for AccountValues {
  fn from(account: &cala_ledger::account::Account) -> Self {
    let values = account.values().clone();
    Self {
      id: values.id.to_string(),
      code: values.code,
      name: values.name,
      tags: values
        .tags
        .into_iter()
        .map(|tag| tag.into_inner())
        .collect(),
      external_id: values.external_id,
      description: values.description,
      metadata: values.metadata,
    }
  }
}

impl From<cala_ledger::account::Account> for AccountValues {
  fn from(account: cala_ledger::account::Account) -> Self {
    let values = account.into_values();
    Self {
      id: values.id.to_string(),
      code: values.code,
      name: values.name,
      tags: values
        .tags
        .into_iter()
        .map(|tag| tag.into_inner())
        .collect(),
      external_id: values.external_id,
      description: values.description,
      metadata: values.metadata,
    }
  }
}
