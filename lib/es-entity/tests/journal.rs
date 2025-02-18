use derive_builder::Builder;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use es_entity::*;

es_entity::entity_id! { JournalId }

#[derive(EsEvent, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[es_event(id = "JournalId")]
pub enum JournalEvent {
    Initialized { id: JournalId },
}

#[derive(EsEntity, Builder)]
#[builder(pattern = "owned", build_fn(error = "EsEntityError"))]
pub struct Journal {
    pub id: JournalId,
    pub events: EntityEvents<JournalEvent>,
}

impl TryFromEvents<JournalEvent> for Journal {
    fn try_from_events(events: EntityEvents<JournalEvent>) -> Result<Self, EsEntityError> {
        let mut builder = JournalBuilder::default();
        for event in events.iter_all() {
            match event {
                JournalEvent::Initialized { id } => {
                    builder = builder.id(*id);
                }
            }
        }
        builder.events(events).build()
    }
}

/// Representation of a new ledger journal entity
/// with required/optional properties and a builder.
#[derive(Debug, Builder)]
pub struct NewJournal {
    #[builder(setter(into))]
    pub id: JournalId,
}

impl NewJournal {
    pub fn builder() -> NewJournalBuilder {
        NewJournalBuilder::default()
    }

    pub fn data_source(&self) -> JournalId {
        self.id
    }
}

impl IntoEvents<JournalEvent> for NewJournal {
    fn into_events(self) -> EntityEvents<JournalEvent> {
        EntityEvents::init(self.id, [JournalEvent::Initialized { id: self.id }])
    }
}

#[derive(Error, Debug)]
pub enum JournalError {
    #[error("JournalError - Sqlx: {0}")]
    Sqlx(#[from] sqlx::Error),
    #[error("JournalError - EsEntityError: {0}")]
    EsEntityError(es_entity::EsEntityError),
    #[error("JournalError - CursorDestructureError: {0}")]
    CursorDestructureError(#[from] es_entity::CursorDestructureError),
    #[error("JournalError - code already exists")]
    CodeAlreadyExists,
}
es_entity::from_es_entity_error!(JournalError);
