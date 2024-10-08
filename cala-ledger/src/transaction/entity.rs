use derive_builder::Builder;
use serde::{Deserialize, Serialize};

pub use cala_types::{primitives::TransactionId, transaction::*};

use crate::{entity::*, primitives::*};

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
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

impl EntityEvent for TransactionEvent {
    type EntityId = TransactionId;
    fn event_table_name() -> &'static str {
        "cala_transaction_events"
    }
}

#[derive(Builder)]
#[builder(pattern = "owned", build_fn(error = "EntityError"))]
pub struct Transaction {
    values: TransactionValues,
    pub(super) events: EntityEvents<TransactionEvent>,
}

impl Entity for Transaction {
    type Event = TransactionEvent;
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
        Self::try_from(events).expect("Failed to build transaction from events")
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
            .entity_first_persisted_at
            .expect("No persisted events")
    }

    pub fn modified_at(&self) -> chrono::DateTime<chrono::Utc> {
        self.events
            .latest_event_persisted_at
            .expect("No events for account")
    }
}

impl TryFrom<EntityEvents<TransactionEvent>> for Transaction {
    type Error = EntityError;

    fn try_from(events: EntityEvents<TransactionEvent>) -> Result<Self, Self::Error> {
        let mut builder = TransactionBuilder::default();
        for event in events.iter() {
            match event {
                #[cfg(feature = "import")]
                TransactionEvent::Imported { source: _, values } => {
                    builder = builder.values(values.clone());
                }
                TransactionEvent::Initialized { values } => {
                    builder = builder.values(values.clone());
                }
            }
        }
        builder.events(events).build()
    }
}

#[derive(Builder, Debug)]
#[allow(dead_code)]
pub(crate) struct NewTransaction {
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

    #[allow(dead_code)]
    pub(super) fn initial_events(self) -> EntityEvents<TransactionEvent> {
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
