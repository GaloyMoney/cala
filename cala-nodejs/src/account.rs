use super::query::*;

#[napi(object)]
pub struct NewAccount {
  pub id: Option<String>,
  pub code: String,
  pub name: String,
  pub external_id: Option<String>,
  pub description: Option<String>,
  pub tags: Option<Vec<String>>,
  pub metadata: Option<serde_json::Value>,
}

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

impl From<cala_ledger::account::Account> for AccountValues {
  fn from(account: cala_ledger::account::Account) -> Self {
    let values = account.values;
    Self {
      id: values.id.to_string(),
      code: values.code,
      name: values.name,
      tags: values.tags,
      external_id: values.external_id,
      description: values.description,
      metadata: values.metadata,
    }
  }
}

#[napi(object)]
pub struct PaginatedAccounts {
  pub accounts: Vec<AccountValues>,
  pub has_next_page: bool,
  pub end_cursor: Option<CursorToken>,
}

#[napi]
pub struct CalaAccounts {
  inner: cala_ledger::account::Accounts,
}

#[napi]
impl CalaAccounts {
  pub fn new(inner: &cala_ledger::account::Accounts) -> Self {
    Self {
      inner: inner.clone(),
    }
  }

  #[napi]
  pub async fn create(&self, new_account: NewAccount) -> napi::Result<String> {
    let id = if let Some(id) = new_account.id {
      id.parse::<cala_ledger::AccountId>()
        .map_err(crate::generic_napi_error)?
    } else {
      cala_ledger::AccountId::new()
    };

    let mut new = cala_ledger::account::NewAccount::builder();
    new.id(id).code(new_account.code).name(new_account.name);

    if let Some(external_id) = new_account.external_id {
      new.external_id(external_id);
    }

    if let Some(description) = new_account.description {
      new.description(description);
    }

    if let Some(tags) = new_account.tags {
      new.tags(tags);
    }

    if let Some(metadata) = new_account.metadata {
      new.metadata(metadata).map_err(crate::generic_napi_error)?;
    }

    let id = self
      .inner
      .create(new.build().map_err(crate::generic_napi_error)?)
      .await
      .map_err(crate::generic_napi_error)?;

    Ok(id.to_string())
  }

  #[napi]
  pub async fn list(&self, query: PaginatedQueryArgs) -> napi::Result<PaginatedAccounts> {
    let query = cala_types::query::PaginatedQueryArgs {
      after: query.after.map(|c| c.try_into()).transpose()?,
      first: usize::try_from(query.first).map_err(crate::generic_napi_error)?,
    };
    let ret = self
      .inner
      .list(query)
      .await
      .map_err(crate::generic_napi_error)?;
    Ok(PaginatedAccounts {
      accounts: ret.entities.into_iter().map(AccountValues::from).collect(),
      has_next_page: ret.has_next_page,
      end_cursor: ret.end_cursor.map(|c| c.into()),
    })
  }
}
