use derive_builder::Builder;
use serde::{Deserialize, Serialize};

crate::entity_id! { OutboxEventId }

pub type WithoutAugmentation = ();

#[derive(Builder, Debug, Serialize, Deserialize)]
#[builder(pattern = "owned")]
pub struct OutboxEvent<T> {
    pub id: OutboxEventId,
    pub sequence: EventSequence,
    pub payload: OutboxEventPayload,
    pub recorded_at: chrono::DateTime<chrono::Utc>,
    #[builder(default)]
    #[serde(skip)]
    pub augmentation: Option<T>,
}

impl<T> OutboxEvent<T> {
    pub fn builder() -> OutboxEventBuilder<T> {
        OutboxEventBuilder::default().id(OutboxEventId::new())
    }
}

impl Clone for OutboxEvent<WithoutAugmentation> {
    fn clone(&self) -> Self {
        Self {
            id: self.id,
            sequence: self.sequence,
            payload: self.payload.clone(),
            recorded_at: self.recorded_at,
            augmentation: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum OutboxEventPayload {}

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
