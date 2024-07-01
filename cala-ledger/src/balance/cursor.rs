use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct BalanceHistoryAsOfCursor {
    pub as_of_version: u32,
    pub current_version: u32,
}
