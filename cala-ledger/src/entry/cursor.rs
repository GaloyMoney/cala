use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use cala_types::primitives::EntryId;

use super::Entry;

#[derive(Debug, Serialize, Deserialize)]
pub struct EntryByCreatedAtCursor {
    pub created_at: DateTime<Utc>,
    pub id: EntryId,
    pub sequence: u32,
}

impl From<&Entry> for EntryByCreatedAtCursor {
    fn from(entry: &Entry) -> Self {
        Self {
            created_at: entry.created_at(),
            id: entry.values().id,
            sequence: entry.values().sequence,
        }
    }
}
