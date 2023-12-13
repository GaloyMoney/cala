use serde::{Deserialize, Serialize};

use super::primitives::*;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct JournalValues {
    pub id: JournalId,
    pub name: String,
    pub external_id: Option<String>,
    pub status: Status,
    pub description: Option<String>,
}
