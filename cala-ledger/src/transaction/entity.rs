use serde::{Deserialize, Serialize};

use crate::primitives::*;
pub use cala_types::{primitives::TransactionId, transaction::*};
use es_entity::*;

#[derive(EsEvent, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
#[serde(tag = "type", rename_all = "snake_case")]
#[es_event(id = "TransactionId", event_context = false)]
pub enum TransactionEvent {
    Initialized {
        values: TransactionValues,
    },
    Updated {
        values: TransactionValues,
        fields: Vec<String>,
    },
}

#[derive(EsEntity)]
pub struct Transaction {
    pub id: TransactionId,
    values: TransactionValues,
    events: EntityEvents<TransactionEvent>,
}

impl Transaction {
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

    pub fn metadata<T: serde::de::DeserializeOwned>(&self) -> Result<Option<T>, serde_json::Error> {
        match &self.values.metadata {
            Some(metadata) => Ok(Some(serde_json::from_value(metadata.clone())?)),
            None => Ok(None),
        }
    }
}

impl TryFromEvents<TransactionEvent> for Transaction {
    fn try_from_events(
        events: EntityEvents<TransactionEvent>,
    ) -> Result<Self, EntityHydrationError> {
        let mut hydrator = Transaction::hydrator();
        for event in events.iter_all() {
            match event {
                TransactionEvent::Initialized { values } => {
                    hydrator.id(values.id).values(values.clone());
                }
                TransactionEvent::Updated { values, .. } => {
                    hydrator.id(values.id).values(values.clone());
                }
            }
        }
        hydrator.build(events)
    }
}

#[derive(bon::Builder, Debug)]
#[allow(dead_code)]
pub struct NewTransaction {
    #[builder(into)]
    pub(super) id: TransactionId,
    pub(super) created_at: chrono::DateTime<chrono::Utc>,
    #[builder(into)]
    pub(super) journal_id: JournalId,
    #[builder(into)]
    pub(super) tx_template_id: TxTemplateId,
    pub(super) effective: chrono::NaiveDate,
    #[builder(into, default = id.to_string())]
    pub(super) correlation_id: String,
    #[builder(into)]
    pub(super) external_id: Option<String>,
    #[builder(into)]
    pub(super) description: Option<String>,
    pub(super) metadata: Option<serde_json::Value>,
    pub(super) entry_ids: Vec<EntryId>,
}

impl IntoEvents<TransactionEvent> for NewTransaction {
    type IdSource = FromCaller;

    fn into_events(
        self,
        builder: EntityEventsBuilder<TransactionEvent, FromCaller>,
    ) -> EntityEvents<TransactionEvent> {
        builder.initial_events(
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_builds() {
        let id = uuid::Uuid::now_v7();
        let new_transaction = NewTransaction::builder()
            .id(id)
            .created_at(chrono::Utc::now())
            .journal_id(uuid::Uuid::now_v7())
            .tx_template_id(uuid::Uuid::now_v7())
            .entry_ids(vec![EntryId::new()])
            .effective(chrono::NaiveDate::default())
            .build();
        assert_eq!(id.to_string(), new_transaction.correlation_id);
        assert!(new_transaction.external_id.is_none());
    }

    #[test]
    fn accepts_metadata() {
        use serde_json::json;
        let new_transaction = NewTransaction::builder()
            .id(uuid::Uuid::now_v7())
            .created_at(chrono::Utc::now())
            .journal_id(uuid::Uuid::now_v7())
            .tx_template_id(uuid::Uuid::now_v7())
            .effective(chrono::NaiveDate::default())
            .metadata(json!({"foo": "bar"}))
            .entry_ids(vec![EntryId::new()])
            .build();
        assert_eq!(new_transaction.metadata, Some(json!({"foo": "bar"})));
    }
}
