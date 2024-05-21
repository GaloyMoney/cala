use serde::{Deserialize, Serialize};

use super::primitives::*;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct JournalValues {
    pub id: JournalId,
    pub version: u32,
    pub name: String,
    pub status: Status,
    pub description: Option<String>,
}
