mod helpers;

use chrono::{DateTime, NaiveDate, Utc};
use es_entity::clock::ClockHandle;
use futures::StreamExt;
use rand::distr::{Alphanumeric, SampleString};

use std::{collections::HashSet, sync::Arc};

use cala_ledger::{outbox::OutboxArchiveConfig, *};

const DAY: std::time::Duration = std::time::Duration::from_secs(24 * 60 * 60);

fn day(n: u32) -> DateTime<Utc> {
    // 2026-07-20 is "day 0"; return noon of day n to stay clear of
    // midnight boundaries.
    NaiveDate::from_ymd_opt(2026, 7, 20)
        .unwrap()
        .checked_add_days(chrono::Days::new(n as u64))
        .unwrap()
        .and_hms_opt(12, 0, 0)
        .unwrap()
        .and_utc()
}

/// A dedicated database for the test. The outbox tables are shared by
/// every test in this crate, and other tests write events with
/// past-dated (manual-clock) `recorded_at` — under a shared table the
/// date-bucketed archive spans and their sequence-range pruning cannot
/// be reasoned about, so this test needs a database of its own.
async fn init_isolated_pool() -> anyhow::Result<(sqlx::PgPool, String)> {
    let pg_con = std::env::var("PG_CON")?;
    let (base, _) = pg_con.rsplit_once('/').expect("PG_CON has a database path");

    let admin = sqlx::PgPool::connect(&format!("{base}/postgres")).await?;
    let db_name = format!(
        "cala_outbox_archive_{}",
        Alphanumeric
            .sample_string(&mut rand::rng(), 8)
            .to_lowercase()
    );
    sqlx::query(&format!("CREATE DATABASE {db_name}"))
        .execute(&admin)
        .await?;
    admin.close().await;

    let pool = sqlx::PgPool::connect(&format!("{base}/{db_name}")).await?;
    sqlx::migrate!().run(&pool).await?;
    Ok((pool, db_name))
}

async fn drop_database(db_name: &str) -> anyhow::Result<()> {
    let pg_con = std::env::var("PG_CON")?;
    let (base, _) = pg_con.rsplit_once('/').expect("PG_CON has a database path");
    let admin = sqlx::PgPool::connect(&format!("{base}/postgres")).await?;
    sqlx::query(&format!("DROP DATABASE {db_name} WITH (FORCE)"))
        .execute(&admin)
        .await?;
    Ok(())
}

async fn create_account(cala: &CalaLedger) -> account::Account {
    let (account, _) = helpers::test_accounts();
    cala.accounts()
        .create(account)
        .await
        .expect("create account")
}

async fn old_event_count(pool: &sqlx::PgPool, before: DateTime<Utc>) -> anyhow::Result<i64> {
    let (count,): (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM cala_persistent_outbox_events WHERE recorded_at < $1")
            .bind(before)
            .fetch_one(pool)
            .await?;
    Ok(count)
}

#[tokio::test]
async fn outbox_archive_sweeps_settled_days_and_replays_across_seam() -> anyhow::Result<()> {
    let (pool, db_name) = init_isolated_pool().await?;
    let (clock, controller) = ClockHandle::manual_at(day(0));

    let storage = Arc::new(obix::InMemoryArchiveStorage::new());
    let archive = OutboxArchiveConfig::new(storage.clone()).with_retention_days(2);

    let cala = CalaLedger::init(
        CalaLedgerConfig::builder()
            .pool(pool.clone())
            .exec_migrations(false)
            .clock(clock)
            .outbox_archive(archive)
            .build()?,
    )
    .await?;

    // Days 0 and 1 fall inside the archive window; day 3 is "today".
    let journal = cala.journals().create(helpers::test_journal()).await?;
    let day0_account = create_account(&cala).await;
    controller.advance(DAY).await;
    let day1_account = create_account(&cala).await;
    controller.advance(DAY).await;
    controller.advance(DAY).await;
    let day3_account = create_account(&cala).await;

    let job_config = job::JobSvcConfig::builder()
        .pool(pool.clone())
        .build()
        .unwrap();
    let mut jobs = job::Jobs::init(job_config).await?;
    cala.register_outbox_archiver(&mut jobs)
        .await
        .map_err(|e| anyhow::anyhow!(e.to_string()))?;
    jobs.start_poll().await?;

    // The job sweeps one settled day per run, rescheduling while catching
    // up: eventually nothing recorded before day 2 remains in postgres...
    let start = std::time::Instant::now();
    while old_event_count(&pool, day(2)).await? > 0 {
        if start.elapsed() > std::time::Duration::from_secs(30) {
            panic!("archiver job did not sweep settled days within timeout");
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    // ...while the day 3 events stay.
    let (recent_count,): (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM cala_persistent_outbox_events WHERE recorded_at >= $1",
    )
    .bind(day(2))
    .fetch_one(&pool)
    .await?;
    assert!(recent_count > 0);

    // The manifest records the exported chunks under the configured prefix.
    let paths: Vec<(String,)> =
        sqlx::query_as("SELECT path FROM cala_persistent_outbox_archive_chunks")
            .fetch_all(&pool)
            .await?;
    assert!(!paths.is_empty());
    assert!(
        paths
            .iter()
            .all(|(path,)| path.starts_with("outbox-archive/cala/")),
        "unexpected chunk paths: {paths:?}"
    );
    assert!(!storage.list().is_empty());

    // A listener resuming from the beginning is served from the archive
    // first, then crosses into postgres mid-stream — day 0/1 events (now
    // only in storage) and day 3 events (still in pg) all arrive, in
    // contiguous sequence order.
    let mut listener = cala.register_outbox_listener(Some(obix::EventSequence::BEGIN));
    let targets: HashSet<uuid::Uuid> = [
        journal.id().into(),
        day0_account.id().into(),
        day1_account.id().into(),
        day3_account.id().into(),
    ]
    .into_iter()
    .collect();
    let mut seen = HashSet::new();
    let mut expected_sequence = 1u64;
    while seen.len() < targets.len() {
        let event = tokio::time::timeout(std::time::Duration::from_secs(30), listener.next())
            .await
            .expect("timed out waiting for replayed event")
            .expect("stream ended during replay")
            .expect("undecodable event during replay");
        assert_eq!(
            u64::from(event.sequence),
            expected_sequence,
            "replay must be contiguous"
        );
        expected_sequence += 1;
        let id = match event.payload.as_ref() {
            Some(outbox::OutboxEventPayload::JournalCreated { journal }) => {
                Some(uuid::Uuid::from(journal.id))
            }
            Some(outbox::OutboxEventPayload::AccountCreated { account }) => {
                Some(uuid::Uuid::from(account.id))
            }
            _ => None,
        };
        if let Some(id) = id {
            if targets.contains(&id) {
                seen.insert(id);
            }
        }
    }
    assert_eq!(seen, targets);

    jobs.shutdown()
        .await
        .map_err(|e| anyhow::anyhow!(e.to_string()))?;
    pool.close().await;
    drop_database(&db_name).await?;
    Ok(())
}
