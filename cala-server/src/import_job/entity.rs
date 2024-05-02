use cala_ledger::entity::*;
use derive_builder::Builder;
use serde::{Deserialize, Serialize};

use super::config::*;
use crate::primitives::*;

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ImportJobEvent {
    Initialized {
        id: ImportJobId,
        name: String,
        description: Option<String>,
        import_config: ImportConfig,
    },
}

impl EntityEvent for ImportJobEvent {
    type EntityId = ImportJobId;
    fn event_table_name() -> &'static str {
        "import_job_events"
    }
}

#[derive(Builder)]
#[builder(pattern = "owned", build_fn(error = "EntityError"))]
pub struct ImportJob {
    pub(super) _events: EntityEvents<ImportJobEvent>,
}

impl TryFrom<EntityEvents<ImportJobEvent>> for ImportJob {
    type Error = EntityError;

    fn try_from(events: EntityEvents<ImportJobEvent>) -> Result<Self, Self::Error> {
        let builder = ImportJobBuilder::default();
        builder._events(events).build()
    }
}

#[derive(Builder, Debug)]
pub struct NewImportJob {
    #[builder(setter(into))]
    pub id: ImportJobId,
    #[builder(setter(into))]
    pub(super) name: String,
    #[builder(setter(into), default)]
    pub(super) description: Option<String>,
    #[builder(setter(into))]
    pub(super) import_config: ImportConfig,
}

impl NewImportJob {
    pub fn builder() -> NewImportJobBuilder {
        NewImportJobBuilder::default()
    }

    pub(super) fn initial_events(self) -> EntityEvents<ImportJobEvent> {
        let id = ImportJobId::new();
        EntityEvents::init(
            id,
            [ImportJobEvent::Initialized {
                id,
                name: self.name,
                description: self.description,
                import_config: self.import_config,
            }],
        )
    }
}
