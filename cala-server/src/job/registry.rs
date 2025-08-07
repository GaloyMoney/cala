use std::collections::HashMap;

use cala_ledger::CalaLedger;

use super::{entity::*, error::JobError, traits::*};

pub struct JobRegistry {
    ledger: CalaLedger,
    initializers: HashMap<JobType, Box<dyn JobInitializer>>,
    retry_settings: HashMap<JobType, RetrySettings>,
}

impl JobRegistry {
    pub(crate) fn new(ledger: &CalaLedger) -> Self {
        Self {
            ledger: ledger.clone(),
            initializers: HashMap::new(),
            retry_settings: HashMap::new(),
        }
    }

    pub fn add_initializer<I: JobInitializer + Default>(&mut self) {
        let job_type = <I as JobInitializer>::job_type();
        assert!(
            !self.initializers.contains_key(&job_type),
            "Job initializer {job_type} already exists",
        );

        self.initializers
            .insert(<I as JobInitializer>::job_type(), Box::<I>::default());
        self.retry_settings.insert(
            <I as JobInitializer>::job_type(),
            <I as JobInitializer>::retry_on_error_settings(),
        );
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

    pub(super) fn retry_settings(&self, job_type: &JobType) -> &RetrySettings {
        self.retry_settings
            .get(job_type)
            .expect("Retry settings not found")
    }
}
