use std::collections::HashMap;

use cala_ledger::CalaLedger;

use super::{entity::*, error::JobError, traits::*};

pub struct JobRegistry {
    ledger: CalaLedger,
    initializers: HashMap<JobType, Box<dyn JobInitializer>>,
}

impl JobRegistry {
    pub(crate) fn new(ledger: &CalaLedger) -> Self {
        Self {
            ledger: ledger.clone(),
            initializers: HashMap::new(),
        }
    }

    pub fn add_initializer<I: JobInitializer + Default>(&mut self) {
        self.initializers
            .insert(<I as JobInitializer>::job_type(), Box::<I>::default());
    }

    pub(super) fn initializer_exists(&self, job_type: &JobType) -> bool {
        self.initializers.contains_key(job_type)
    }

    pub(super) fn init_job(&self, job: Job) -> Result<Box<dyn JobRunner>, JobError> {
        self.initializers
            .get(&job.job_type)
            .ok_or(JobError::NoInitializerPresent)?
            .init(job, &self.ledger)
            .map_err(|e| JobError::JobInitError(e.to_string()))
    }
}
