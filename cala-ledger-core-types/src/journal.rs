use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::primitives::*;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct JournalValues {
    pub id: JournalId,
    pub version: u32,
    pub created_at: DateTime<Utc>,
    pub modified_at: DateTime<Utc>,
    pub name: String,
    pub status: Status,
    pub description: Option<String>,
}
