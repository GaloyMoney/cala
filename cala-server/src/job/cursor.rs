use serde::{Deserialize, Serialize};

use super::entity::*;
use crate::primitives::JobId;

#[derive(Debug, Serialize, Deserialize)]
pub struct JobByNameCursor {
    pub name: String,
    pub id: JobId,
}

impl From<Job> for JobByNameCursor {
    fn from(job: Job) -> Self {
        Self {
            name: job.name.clone(),
            id: job.id,
        }
    }
}
