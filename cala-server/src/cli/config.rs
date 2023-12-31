use anyhow::Context;
use cala_tracing::TracingConfig;
use serde::{Deserialize, Serialize};

use std::path::Path;

use super::db::*;
use crate::server::ServerConfig;

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub db: DbConfig,
    #[serde(default)]
    pub server: ServerConfig,
    // #[serde(default)]
    // pub app: AppConfig,
    #[serde(default = "default_tracing_config")]
    pub tracing: TracingConfig,
}

fn default_tracing_config() -> TracingConfig {
    TracingConfig {
        service_name: "cala-server".to_string(),
    }
}

pub struct EnvOverride {
    pub db_con: String,
}

impl Config {
    pub fn from_path(
        path: impl AsRef<Path>,
        EnvOverride { db_con }: EnvOverride,
    ) -> anyhow::Result<Self> {
        let config_file = std::fs::read_to_string(path).context("Couldn't read config file")?;
        let mut config: Config =
            serde_yaml::from_str(&config_file).context("Couldn't parse config file")?;
        config.db.pg_con = db_con;

        Ok(config)
    }
}
