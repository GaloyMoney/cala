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

    pub fn add_initializer(&mut self, job_type: JobType, initializer: Box<dyn JobInitializer>) {
        self.initializers.insert(job_type, initializer);
    }

    pub(super) fn initializer_exists(&self, job_type: &JobType) -> bool {
        self.initializers.contains_key(job_type)
    }

    pub(super) fn init_job(&self, job: &Job) -> Result<Box<dyn JobRunner>, JobError> {
        self.initializers
            .get(&job.job_type)
            .expect("no initializer present")
            .init(job, &self.ledger)
            .map_err(|e| JobError::JobInitError(e.to_string()))
    }
}
