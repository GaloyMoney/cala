use serde::{Deserialize, Serialize};

use crate::{account::*, primitives::*};

#[derive(Debug, Serialize, Deserialize)]
pub struct OutboxEvent {
    pub id: OutboxEventId,
    pub sequence: EventSequence,
    pub payload: OutboxEventPayload,
    pub recorded_at: chrono::DateTime<chrono::Utc>,
}

impl Clone for OutboxEvent {
    fn clone(&self) -> Self {
        Self {
            id: self.id,
            sequence: self.sequence,
            payload: self.payload.clone(),
            recorded_at: self.recorded_at,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum OutboxEventPayload {
    AccountCreated { account: AccountValues },
}

#[derive(
    sqlx::Type, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Copy, Clone, Serialize, Deserialize,
)]
#[serde(transparent)]
#[sqlx(transparent)]
pub struct EventSequence(i64);
impl EventSequence {
    pub(super) const BEGIN: Self = EventSequence(0);
    pub(super) fn next(&self) -> Self {
        Self(self.0 + 1)
    }
}

impl From<u64> for EventSequence {
    fn from(n: u64) -> Self {
        Self(n as i64)
    }
}

impl From<EventSequence> for u64 {
    fn from(EventSequence(n): EventSequence) -> Self {
        n as u64
    }
}

impl std::fmt::Display for EventSequence {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
