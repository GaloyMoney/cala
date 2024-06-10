use serde::{Deserialize, Serialize};

use crate::job::JobExecutorConfig;

use super::EncryptionConfig;

#[derive(Clone, Default, Debug, Deserialize, Serialize)]
pub struct AppConfig {
    #[serde(default)]
    pub job_execution: JobExecutorConfig,
    #[serde(default)]
    pub encryption: EncryptionConfig,
}
