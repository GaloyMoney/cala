use serde::{Deserialize, Serialize};

use crate::jobs::JobExecutorConfig;

#[derive(Clone, Default, Debug, Deserialize, Serialize)]
pub struct AppConfig {
    #[serde(default)]
    pub job_execution: JobExecutorConfig,
}
