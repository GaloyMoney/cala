use cala_ledger::{entity::*, CalaLedger};
use derive_builder::Builder;
use serde::{Deserialize, Serialize};

use super::{cala_outbox::*, config::*};
use crate::{
    job::{JobRunner, JobTemplate},
    primitives::*,
};

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ImportJobEvent {
    Initialized {
        id: ImportJobId,
        name: String,
        description: Option<String>,
        config: ImportJobConfig,
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
    pub id: ImportJobId,
    pub name: String,
    pub description: Option<String>,
    config: ImportJobConfig,
    pub(super) _events: EntityEvents<ImportJobEvent>,
}

impl ImportJob {
    pub fn runner(&self, ledger: &CalaLedger) -> Box<dyn JobRunner> {
        let ImportJobConfig::CalaOutbox(config) = &self.config;
        Box::new(CalaOutboxImportJob::new(config.clone(), ledger))
    }
}

impl From<&ImportJob> for JobTemplate {
    fn from(job: &ImportJob) -> Self {
        JobTemplate::new(CALA_OUTBOX_IMPORT_JOB_TYPE, job.id)
    }
}

impl Entity for ImportJob {
    type Event = ImportJobEvent;
}

impl TryFrom<EntityEvents<ImportJobEvent>> for ImportJob {
    type Error = EntityError;

    fn try_from(events: EntityEvents<ImportJobEvent>) -> Result<Self, Self::Error> {
        let mut builder = ImportJobBuilder::default();
        for event in events.iter() {
            let ImportJobEvent::Initialized {
                id,
                name,
                description,
                config,
            } = event;
            builder = builder
                .id(*id)
                .name(name.clone())
                .description(description.clone())
                .config(config.clone());
        }
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
    pub(super) config: ImportJobConfig,
}

impl NewImportJob {
    pub fn builder() -> NewImportJobBuilder {
        let mut builder = NewImportJobBuilder::default();
        builder.id(ImportJobId::new());
        builder
    }

    pub(super) fn initial_events(self) -> EntityEvents<ImportJobEvent> {
        EntityEvents::init(
            self.id,
            [ImportJobEvent::Initialized {
                id: self.id,
                name: self.name,
                description: self.description,
                config: self.config,
            }],
        )
    }
}
