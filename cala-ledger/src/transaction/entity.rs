use derive_builder::Builder;
use serde::{Deserialize, Serialize};

use crate::primitives::*;
pub use cala_types::{primitives::TransactionId, transaction::*};
use es_entity::*;

#[derive(EsEvent, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[es_event(id = "TransactionId")]
pub enum TransactionEvent {
    #[cfg(feature = "import")]
    Imported {
        source: DataSource,
        values: TransactionValues,
    },
    Initialized {
        values: TransactionValues,
    },
}

#[derive(EsEntity, Builder)]
#[builder(pattern = "owned", build_fn(error = "EsEntityError"))]
pub struct Transaction {
    pub id: TransactionId,
    values: TransactionValues,
    events: EntityEvents<TransactionEvent>,
}

impl Transaction {
    #[cfg(feature = "import")]
    pub(super) fn import(source: DataSourceId, values: TransactionValues) -> Self {
        let events = EntityEvents::init(
            values.id,
            [TransactionEvent::Imported {
                source: DataSource::Remote { id: source },
                values,
            }],
        );
        Self::try_from_events(events).expect("Failed to build transaction from events")
    }

    pub fn id(&self) -> TransactionId {
        self.values.id
    }

    pub fn journal_id(&self) -> JournalId {
        self.values.journal_id
    }

    pub fn values(&self) -> &TransactionValues {
        &self.values
    }

    pub fn into_values(self) -> TransactionValues {
        self.values
    }

    pub fn created_at(&self) -> chrono::DateTime<chrono::Utc> {
        self.events
            .entity_first_persisted_at()
            .expect("No persisted events")
    }

    pub fn effective(&self) -> chrono::NaiveDate {
        self.values.effective
    }

    pub fn modified_at(&self) -> chrono::DateTime<chrono::Utc> {
        self.events
            .entity_last_modified_at()
            .expect("Entity not persisted")
    }
}

impl TryFromEvents<TransactionEvent> for Transaction {
    fn try_from_events(events: EntityEvents<TransactionEvent>) -> Result<Self, EsEntityError> {
        let mut builder = TransactionBuilder::default();
        for event in events.iter_all() {
            match event {
                #[cfg(feature = "import")]
                TransactionEvent::Imported { source: _, values } => {
                    builder = builder.id(values.id).values(values.clone());
                }
                TransactionEvent::Initialized { values } => {
                    builder = builder.id(values.id).values(values.clone());
                }
            }
        }
        builder.events(events).build()
    }
}

#[derive(Builder, Debug)]
#[allow(dead_code)]
pub struct NewTransaction {
    #[builder(setter(custom))]
    pub(super) id: TransactionId,
    pub(super) created_at: chrono::DateTime<chrono::Utc>,
    #[builder(setter(into))]
    pub(super) journal_id: JournalId,
    #[builder(setter(into))]
    pub(super) tx_template_id: TxTemplateId,
    pub(super) effective: chrono::NaiveDate,
    #[builder(setter(into), default)]
    pub(super) correlation_id: String,
    #[builder(setter(strip_option, into), default)]
    pub(super) external_id: Option<String>,
    #[builder(setter(strip_option, into), default)]
    pub(super) description: Option<String>,
    #[builder(setter(into), default)]
    pub(super) metadata: Option<serde_json::Value>,
    pub(super) entry_ids: Vec<EntryId>,
}

impl NewTransaction {
    pub fn builder() -> NewTransactionBuilder {
        NewTransactionBuilder::default()
    }

    pub(super) fn data_source(&self) -> DataSource {
        DataSource::Local
    }
}

impl IntoEvents<TransactionEvent> for NewTransaction {
    fn into_events(self) -> EntityEvents<TransactionEvent> {
        EntityEvents::init(
            self.id,
            [TransactionEvent::Initialized {
                values: TransactionValues {
                    id: self.id,
                    version: 1,
                    created_at: self.created_at,
                    modified_at: self.created_at,
                    journal_id: self.journal_id,
                    tx_template_id: self.tx_template_id,
                    effective: self.effective,
                    correlation_id: self.correlation_id,
                    external_id: self.external_id,
                    description: self.description,
                    metadata: self.metadata,
                    entry_ids: self.entry_ids,
                },
            }],
        )
    }
}

impl NewTransactionBuilder {
    pub fn id(&mut self, id: impl Into<TransactionId>) -> &mut Self {
        self.id = Some(id.into());
        if self.correlation_id.is_none() {
            self.correlation_id = Some(self.id.unwrap().to_string());
        }
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_builds() {
        let id = uuid::Uuid::new_v4();
        let new_transaction = NewTransaction::builder()
            .id(id)
            .created_at(chrono::Utc::now())
            .journal_id(uuid::Uuid::new_v4())
            .tx_template_id(uuid::Uuid::new_v4())
            .entry_ids(vec![EntryId::new()])
            .effective(chrono::NaiveDate::default())
            .build()
            .unwrap();
        assert_eq!(id.to_string(), new_transaction.correlation_id);
        assert!(new_transaction.external_id.is_none());
    }

    #[test]
    fn fails_when_mandatory_fields_are_missing() {
        let new_transaction = NewTransaction::builder().build();
        assert!(new_transaction.is_err());
    }

    #[test]
    fn accepts_metadata() {
        use serde_json::json;
        let new_transaction = NewTransaction::builder()
            .id(uuid::Uuid::new_v4())
            .created_at(chrono::Utc::now())
            .journal_id(uuid::Uuid::new_v4())
            .tx_template_id(uuid::Uuid::new_v4())
            .effective(chrono::NaiveDate::default())
            .metadata(json!({"foo": "bar"}))
            .entry_ids(vec![EntryId::new()])
            .build()
            .unwrap();
        assert_eq!(new_transaction.metadata, Some(json!({"foo": "bar"})));
    }
}
