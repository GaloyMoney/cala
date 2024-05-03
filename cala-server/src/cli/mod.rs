pub mod config;
mod db;

use anyhow::Context;
use clap::Parser;
use std::{fs, path::PathBuf};

use self::config::{Config, EnvOverride};

#[derive(Parser)]
#[clap(long_about = None)]
struct Cli {
    #[clap(
        short,
        long,
        env = "CALA_CONFIG",
        default_value = "cala.yml",
        value_name = "FILE"
    )]
    config: PathBuf,
    #[clap(
        long,
        env = "CALA_HOME",
        default_value = ".cala",
        value_name = "DIRECTORY"
    )]
    cala_home: String,
    #[clap(env = "PG_CON")]
    pg_con: String,
}

pub async fn run() -> anyhow::Result<()> {
    let cli = Cli::parse();

    let config = Config::from_path(cli.config, EnvOverride { db_con: cli.pg_con })?;

    run_cmd(&cli.cala_home, config).await?;

    Ok(())
}

async fn run_cmd(cala_home: &str, config: Config) -> anyhow::Result<()> {
    use cala_ledger::{CalaLedger, CalaLedgerConfig};
    cala_tracing::init_tracer(config.tracing)?;
    store_server_pid(cala_home, std::process::id())?;
    let pool = db::init_pool(&config.db).await?;
    let ledger_config = CalaLedgerConfig::builder().pool(pool.clone()).build()?;
    let ledger = CalaLedger::init(ledger_config).await?;
    let app = crate::app::CalaApp::new(pool, ledger);
    crate::server::run(config.server, app).await?;
    Ok(())
}

pub fn store_server_pid(cala_home: &str, pid: u32) -> anyhow::Result<()> {
    create_cala_dir(cala_home)?;
    let _ = fs::remove_file(format!("{cala_home}/server-pid"));
    fs::write(format!("{cala_home}/server-pid"), pid.to_string()).context("Writing PID file")?;
    Ok(())
}

fn create_cala_dir(bria_home: &str) -> anyhow::Result<()> {
    let _ = fs::create_dir(bria_home);
    Ok(())
}
