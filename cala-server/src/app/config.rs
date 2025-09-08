use serde::{Deserialize, Serialize};

use job::JobsConfig;

#[derive(Clone, Default, Debug, Deserialize, Serialize)]
pub struct AppConfig {
    #[serde(default)]
    pub jobs: JobsConfig,
}
