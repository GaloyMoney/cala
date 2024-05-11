use cala_ledger::entity::*;
use derive_builder::Builder;
use serde::{Deserialize, Serialize};

use crate::primitives::JobId;

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum JobEvent {
    Initialized {
        id: JobId,
        name: String,
        description: Option<String>,
        config: serde_json::Value,
    },
}

impl EntityEvent for JobEvent {
    type EntityId = JobId;
    fn event_table_name() -> &'static str {
        "job_events"
    }
}

#[derive(Builder)]
#[builder(pattern = "owned", build_fn(error = "EntityError"))]
pub struct Job {
    pub id: JobId,
    pub name: String,
    pub description: Option<String>,
    config: serde_json::Value,
    pub(super) _events: EntityEvents<JobEvent>,
}

impl Entity for Job {
    type Event = JobEvent;
}

impl TryFrom<EntityEvents<JobEvent>> for Job {
    type Error = EntityError;

    fn try_from(events: EntityEvents<JobEvent>) -> Result<Self, Self::Error> {
        let mut builder = JobBuilder::default();
        for event in events.iter() {
            let JobEvent::Initialized {
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
pub struct NewJob {
    #[builder(setter(into))]
    pub id: JobId,
    #[builder(setter(into))]
    pub(super) name: String,
    #[builder(setter(into), default)]
    pub(super) description: Option<String>,
    #[builder(setter(custom))]
    pub(super) config: serde_json::Value,
}

impl NewJob {
    pub fn builder() -> NewJobBuilder {
        let mut builder = NewJobBuilder::default();
        builder.id(JobId::new());
        builder
    }

    pub(super) fn initial_events(self) -> EntityEvents<JobEvent> {
        EntityEvents::init(
            self.id,
            [JobEvent::Initialized {
                id: self.id,
                name: self.name,
                description: self.description,
                config: self.config,
            }],
        )
    }
}

impl NewJobBuilder {
    pub fn config<C: serde::Serialize>(
        &mut self,
        config: C,
    ) -> Result<&mut Self, serde_json::Error> {
        self.config = Some(serde_json::to_value(config)?);
        Ok(self)
    }
}
