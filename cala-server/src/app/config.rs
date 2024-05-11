use serde::{Deserialize, Serialize};

use crate::job::JobExecutorConfig;

#[derive(Clone, Default, Debug, Deserialize, Serialize)]
pub struct AppConfig {
    #[serde(default)]
    pub job_execution: JobExecutorConfig,
}
