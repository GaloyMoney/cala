//! Calculation rollup lag drain.
//!
//! Creates a fresh database, one journal with effective balances and one
//! `Global` calculation (no condition, no dimensions), posts `POSTINGS`
//! transactions spread over `RECIPIENTS` recipients and `DAYS` effective
//! dates while the job poller is stopped, then starts the poller and times
//! how long the calculation rollup takes to drain the backlog.
//!
//! Run against the dev database (`make start-deps`, then `PG_CON` set):
//!
//! ```text
//! cargo run --release -p cala-perf --example calculation_rollup_lag
//! BENCH_POSTINGS=50000 cargo run --release -p cala-perf --example calculation_rollup_lag
//! ```

use sqlx::Connection;
use std::time::{Duration, Instant};

use cala_ledger::*;
use cala_perf::{init_accounts, init_global_calculation, init_journal, templates::simple_transfer};

const RECIPIENTS: usize = 50;
const DAYS: i64 = 30;
const CONCURRENCY: usize = 8;

fn env_usize(name: &str, default: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

async fn fresh_pool() -> anyhow::Result<sqlx::PgPool> {
    let base = std::env::var("PG_CON")?;
    let db_name = format!("cala_calc_lag_{}", uuid::Uuid::now_v7().simple());
    let mut admin = sqlx::PgConnection::connect(&base).await?;
    sqlx::query(sqlx::AssertSqlSafe(format!(
        r#"CREATE DATABASE "{db_name}""#
    )))
    .execute(&mut admin)
    .await?;
    admin.close().await?;
    let (prefix, _) = base.rsplit_once('/').expect("PG_CON has a database path");
    Ok(sqlx::postgres::PgPoolOptions::new()
        .max_connections(20)
        .connect(&format!("{prefix}/{db_name}"))
        .await?)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let postings = env_usize("BENCH_POSTINGS", 10_000);

    let pool = fresh_pool().await?;
    let mut jobs =
        job::Jobs::init(job::JobSvcConfig::builder().pool(pool.clone()).build()?).await?;
    let cala = CalaLedger::init(
        CalaLedgerConfig::builder()
            .pool(pool)
            .exec_migrations(true)
            .build()?,
        &mut jobs,
    )
    .await?;
    simple_transfer::init(&cala).await?;
    let journal = init_journal(&cala, true).await?;
    init_global_calculation(&cala, &journal).await?;
    let mut pairs = Vec::with_capacity(RECIPIENTS);
    for _ in 0..RECIPIENTS {
        let (sender, recipient) = init_accounts(&cala, false).await?;
        pairs.push((sender.id(), recipient.id()));
    }
    let today = cala.clock().now().date_naive();

    let started = Instant::now();
    let mut tasks = tokio::task::JoinSet::new();
    for worker in 0..CONCURRENCY {
        let cala = cala.clone();
        let pairs = pairs.clone();
        let journal_id = journal.id();
        tasks.spawn(async move {
            for i in (worker..postings).step_by(CONCURRENCY) {
                let (sender, recipient) = pairs[i % RECIPIENTS];
                let effective = today - chrono::Duration::days(i as i64 % DAYS);
                simple_transfer::execute_effective(&cala, journal_id, sender, recipient, effective)
                    .await?;
            }
            anyhow::Ok(())
        });
    }
    while let Some(result) = tasks.join_next().await {
        result??;
    }
    let posting_time = started.elapsed();

    let status = cala.calculation_rollup_status().await?;
    let backlog = status.lag();
    let started = Instant::now();
    jobs.start_poll().await?;
    status.await_completion(Duration::from_secs(3600)).await?;
    let drain_time = started.elapsed();

    println!("postings:            {postings}");
    println!("posting wall time:   {posting_time:?}");
    println!("backlog (sequences): {backlog}");
    println!("calculation drain:   {drain_time:?}");
    println!(
        "drain throughput:    {:.0} postings/s",
        postings as f64 / drain_time.as_secs_f64()
    );
    Ok(())
}
