#[napi(object)]
pub struct NewJournal {
  pub id: Option<String>,
  pub name: String,
  pub external_id: Option<String>,
  pub description: Option<String>,
}

#[napi(object)]
pub struct JournalValues {
  pub id: String,
  pub name: String,
  pub external_id: Option<String>,
  pub description: Option<String>,
}

#[napi]
pub struct CalaJournals {
  inner: cala_ledger::journal::Journals,
}

#[napi]
impl CalaJournals {
  pub fn new(inner: &cala_ledger::journal::Journals) -> Self {
    Self {
      inner: inner.clone(),
    }
  }

  #[napi]
  pub async fn create(&self, new_journal: NewJournal) -> napi::Result<String> {
    let id = if let Some(id) = new_journal.id {
      id.parse::<cala_ledger::JournalId>()
        .map_err(crate::generic_napi_error)?
    } else {
      cala_ledger::JournalId::new()
    };
    let mut new = cala_ledger::journal::NewJournal::builder();
    new.id(id).name(new_journal.name);
    if let Some(external_id) = new_journal.external_id {
      new.external_id(external_id);
    }
    if let Some(description) = new_journal.description {
      new.description(description);
    }

    let res = self
      .inner
      .create(new.build().map_err(crate::generic_napi_error)?)
      .await
      .map_err(crate::generic_napi_error)?;
    Ok(res.id.to_string())
  }
}
