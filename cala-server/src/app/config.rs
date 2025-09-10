use serde::{Deserialize, Serialize};

use job::JobPollerConfig;

#[derive(Clone, Default, Debug, Deserialize, Serialize)]
pub struct AppConfig {
    #[serde(default)]
    pub jobs: JobPollerConfig,
}
