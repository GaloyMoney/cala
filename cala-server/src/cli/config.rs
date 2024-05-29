use anyhow::Context;
use cala_tracing::TracingConfig;
use serde::{Deserialize, Serialize};

use std::path::Path;

use super::db::*;
use crate::{app::AppConfig, server::ServerConfig};

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub db: DbConfig,
    #[serde(default)]
    pub server: ServerConfig,
    #[serde(default)]
    pub app: AppConfig,
    #[serde(default)]
    pub tracing: TracingConfig,
}

pub struct EnvOverride {
    pub db_con: String,
    pub server_id: Option<String>,
}

impl Config {
    pub fn load_config(
        path: Option<impl AsRef<Path>>,
        env_override: EnvOverride,
    ) -> anyhow::Result<Self> {
        let mut config = if let Some(config_path) = path {
            let config_file =
                std::fs::read_to_string(config_path).context("Couldn't read config file")?;
            serde_yaml::from_str(&config_file).context("Couldn't parse config file")?
        } else {
            println!("No config file provided, using default config.");
            Config::default()
        };

        config.apply_env_override(env_override);
        Ok(config)
    }

    fn apply_env_override(&mut self, EnvOverride { db_con, server_id }: EnvOverride) {
        self.db.pg_con = db_con;
        if let Some(server_id) = server_id {
            self.app.job_execution.server_id.clone_from(&server_id);
            self.tracing.service_instance_id = server_id;
        }
    }
}
