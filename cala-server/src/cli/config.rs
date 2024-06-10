use anyhow::Context;
use cala_tracing::TracingConfig;
use serde::{Deserialize, Serialize};

use std::path::Path;

use super::db::*;
use crate::{app::AppConfig, integration::EncryptionKey, server::ServerConfig};

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
    pub encryption_key: String,
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

        let _ = config.apply_env_override(env_override);
        Ok(config)
    }

    fn apply_env_override(
        &mut self,
        EnvOverride {
            db_con,
            encryption_key,
        }: EnvOverride,
    ) -> anyhow::Result<()> {
        self.db.pg_con = db_con;

        let key_bytes = hex::decode(encryption_key)?;
        if key_bytes.len() != 32 {
            return Err(anyhow::anyhow!(
                "Signer encryption key must be 32 bytes, got {}",
                key_bytes.len()
            ));
        }

        self.app.encryption.key = EncryptionKey::clone_from_slice(key_bytes.as_ref());
        Ok(())
    }
}
