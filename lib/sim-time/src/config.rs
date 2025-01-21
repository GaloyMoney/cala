use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_with::serde_as;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TimeConfig {
    pub(super) realtime: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) sim_time: Option<SimTimeConfig>,
}

impl Default for TimeConfig {
    fn default() -> Self {
        Self {
            realtime: true,
            sim_time: None,
        }
    }
}

#[serde_as]
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SimTimeConfig {
    #[serde(default = "Utc::now")]
    pub(super) start_at: DateTime<Utc>,
    pub(super) tick_interval_ms: u64,
    #[serde_as(as = "serde_with::DurationSeconds<u64>")]
    pub(super) tick_duration_secs: std::time::Duration,
    #[serde(default)]
    pub(super) transform_to_realtime: bool,
}
