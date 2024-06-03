use serde::{Deserialize, Serialize};

use std::time::Duration;

#[serde_with::serde_as]
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct JobExecutorConfig {
    #[serde_as(as = "serde_with::DurationSeconds<u64>")]
    #[serde(default = "default_poll_interval")]
    pub poll_interval: Duration,
}

impl Default for JobExecutorConfig {
    fn default() -> Self {
        Self {
            poll_interval: default_poll_interval(),
        }
    }
}

fn default_poll_interval() -> Duration {
    Duration::from_secs(5)
}
