use serde::{Deserialize, Serialize};

use std::time::Duration;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde_with::serde_as]
pub struct JobExecutorConfig {
    #[serde(default = "random_server_id")]
    pub server_id: String,
    #[serde_as(as = "serde_with::DurationSeconds<u64>")]
    #[serde(default = "default_poll_interval")]
    pub poll_interval: Duration,
}

impl Default for JobExecutorConfig {
    fn default() -> Self {
        Self {
            server_id: random_server_id(),
            poll_interval: default_poll_interval(),
        }
    }
}

fn random_server_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

fn default_poll_interval() -> Duration {
    Duration::from_secs(5)
}
