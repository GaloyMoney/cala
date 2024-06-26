mod values;

use values::*;

#[napi(object)]
pub struct NewJournal {
  pub id: Option<String>,
  pub name: String,
  pub external_id: Option<String>,
  pub description: Option<String>,
}

#[napi]
pub struct CalaJournals {
  inner: cala_ledger::journal::Journals,
}

#[napi]
pub struct CalaJournal {
  inner: cala_ledger::journal::Journal,
}

#[napi]
impl CalaJournal {
  #[napi]
  pub fn id(&self) -> String {
    self.inner.id().to_string()
  }

  #[napi]
  pub fn values(&self) -> JournalValues {
    JournalValues::from(&self.inner)
  }
}

#[napi]
impl CalaJournals {
  pub fn new(inner: &cala_ledger::journal::Journals) -> Self {
    Self {
      inner: inner.clone(),
    }
  }

  #[napi]
  pub async fn create(&self, new_journal: NewJournal) -> napi::Result<CalaJournal> {
    let id = if let Some(id) = new_journal.id {
      id.parse::<cala_ledger::JournalId>()
        .map_err(crate::generic_napi_error)?
    } else {
      cala_ledger::JournalId::new()
    };
    let mut new = cala_ledger::journal::NewJournal::builder();
    new.id(id).name(new_journal.name);
    if let Some(description) = new_journal.description {
      new.description(description);
    }

    let journal = self
      .inner
      .create(new.build().map_err(crate::generic_napi_error)?)
      .await
      .map_err(crate::generic_napi_error)?;
    Ok(CalaJournal { inner: journal })
  }
}
