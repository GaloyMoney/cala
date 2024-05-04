use serde::{Deserialize, Serialize};

use crate::job_execution::JobExecutionConfig;

#[derive(Clone, Default, Debug, Deserialize, Serialize)]
pub struct AppConfig {
    #[serde(default)]
    pub job_execution: JobExecutionConfig,
}
