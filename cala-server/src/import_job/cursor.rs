use serde::{Deserialize, Serialize};

use super::entity::*;
use crate::primitives::ImportJobId;

#[derive(Debug, Serialize, Deserialize)]
pub struct ImportJobByNameCursor {
    pub name: String,
    pub id: ImportJobId,
}

impl From<ImportJob> for ImportJobByNameCursor {
    fn from(job: ImportJob) -> Self {
        Self {
            name: job.name.clone(),
            id: job.id,
        }
    }
}
