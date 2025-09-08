pub mod config;
mod db;

use anyhow::Context;
use clap::Parser;
use std::{fs, path::PathBuf};

use self::config::{Config, EnvOverride};
use crate::extension::*;

#[derive(Parser)]
#[clap(version, long_about = None)]
struct Cli {
    #[clap(short, long, env = "CALA_CONFIG", value_name = "FILE")]
    config: Option<PathBuf>,
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

pub async fn run<Q: QueryExtensionMarker, M: MutationExtensionMarker>() -> anyhow::Result<()> {
    let cli = Cli::parse();

    let config = Config::load_config(cli.config, EnvOverride { db_con: cli.pg_con })?;

    run_cmd::<Q, M>(&cli.cala_home, config).await?;

    Ok(())
}

async fn run_cmd<Q: QueryExtensionMarker, M: MutationExtensionMarker>(
    cala_home: &str,
    config: Config,
) -> anyhow::Result<()> {
    use cala_ledger::{CalaLedger, CalaLedgerConfig};
    cala_tracing::init_tracer(config.tracing)?;
    store_server_pid(cala_home, std::process::id())?;
    let pool = db::init_pool(&config.db).await?;
    let ledger_config = CalaLedgerConfig::builder()
        .pool(pool.clone())
        .exec_migrations(true)
        .build()?;
    let ledger = CalaLedger::init(ledger_config).await?;
    let app = crate::app::CalaApp::run(pool, config.app, ledger).await?;
    crate::server::run::<Q, M>(config.server, app).await?;
    Ok(())
}

pub fn store_server_pid(cala_home: &str, pid: u32) -> anyhow::Result<()> {
    create_cala_dir(cala_home)?;
    let _ = fs::remove_file(format!("{cala_home}/server-pid"));
    fs::write(format!("{cala_home}/server-pid"), pid.to_string()).context("Writing PID file")?;
    Ok(())
}

fn create_cala_dir(cala_home: &str) -> anyhow::Result<()> {
    let _ = fs::create_dir(cala_home);
    Ok(())
}
