use std::collections::HashMap;
use uuid::Uuid;

use super::traits::*;

pub struct JobRegistry {
    initializers: HashMap<JobType, Box<dyn JobInitializer>>,
}

impl JobRegistry {
    pub fn add_initializer<I: JobInitializer + 'static>(
        &mut self,
        job_type: JobType,
        initializer: I,
    ) {
        self.initializers.insert(job_type, Box::new(initializer));
    }
    pub async fn init_job(
        &self,
        job_type: JobType,
        id: Uuid,
    ) -> Result<Box<dyn JobRunner>, Box<dyn std::error::Error>> {
        self.initializers.get(&job_type).unwrap().init(id).await
    }
}
