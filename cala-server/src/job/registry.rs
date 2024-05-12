use uuid::Uuid;

use std::collections::HashMap;

use super::{entity::JobType, error::JobError, traits::*};

pub struct JobRegistry {
    initializers: HashMap<JobType, Box<dyn JobInitializer>>,
}

impl JobRegistry {
    pub fn new() -> Self {
        Self {
            initializers: HashMap::new(),
        }
    }

    pub fn add_initializer(&mut self, job_type: JobType, initializer: Box<dyn JobInitializer>) {
        self.initializers.insert(job_type, initializer);
    }

    pub(super) fn initializer_exists(&self, job_type: &JobType) -> bool {
        self.initializers.contains_key(job_type)
    }

    pub(super) async fn init_job(
        &self,
        job_type: JobType,
        id: Uuid,
    ) -> Result<Box<dyn JobRunner>, JobError> {
        self.initializers
            .get(&job_type)
            .expect("no initializer present")
            .init(id)
            .await
            .map_err(|e| JobError::JobInitError(e.to_string()))
    }
}
