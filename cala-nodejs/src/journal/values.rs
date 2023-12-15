#[napi(object)]
pub struct JournalValues {
    pub id: String,
    pub name: String,
    pub external_id: Option<String>,
    pub description: Option<String>,
}

impl From<&cala_ledger::journal::Journal> for JournalValues {
    fn from(journal: &cala_ledger::journal::Journal) -> Self {
        let values = journal.values().clone();
        Self {
            id: values.id.to_string(),
            name: values.name,
            external_id: values.external_id,
            description: values.description,
        }
    }
}

impl From<cala_ledger::journal::Journal> for JournalValues {
    fn from(journal: cala_ledger::journal::Journal) -> Self {
        let values = journal.into_values();
        Self {
            id: values.id.to_string(),
            name: values.name,
            external_id: values.external_id,
            description: values.description,
        }
    }
}
