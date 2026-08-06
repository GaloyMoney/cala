//! End-to-end tests for the entry-sourced EC verify/repair tool
//! (`Balances::verify_ec` / `Balances::repair_ec`).
//!
//! The reconciler anchors on the streaming rollup job's committed cursor,
//! so — like the rollup tests — every test runs on its own isolated
//! database (`helpers::init_isolated_pool`). Convergence via
//! `wait_for_settled` doubles as a checkpoint barrier: the applier commits
//! its balance writes and its cursor atomically, so once the balance is
//! visible the cursor covers the folded events.

mod helpers;

use rand::distr::{Alphanumeric, SampleString};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use cala_ledger::{
    account::{Account, NewAccount},
    account_set::{AccountSet, AccountSetId, NewAccountSet},
    balance::error::BalanceError,
    job::Jobs,
    journal::NewJournal,
    primitives::BalanceRollup,
    tx_template::Params,
    AccountId, CalaLedger, CalaLedgerConfig, Currency, JournalId, TransactionId,
};

const POST_AMOUNT: Decimal = dec!(7);

struct Fixture {
    cala: CalaLedger,
    journal_id: JournalId,
    sender: Account,
    members: Vec<Account>,
    tx_code: String,
}

async fn setup(pool: sqlx::PgPool, journal: NewJournal) -> anyhow::Result<(Fixture, Jobs)> {
    let mut jobs = helpers::init_jobs(pool.clone()).await?;
    let cala_config = CalaLedgerConfig::builder()
        .pool(pool)
        .exec_migrations(false)
        .build()?;
    let cala = CalaLedger::init(cala_config, &mut jobs).await?;

    let journal = cala.journals().create(journal).await?;

    let sender_code = Alphanumeric.sample_string(&mut rand::rng(), 32);
    let sender = NewAccount::builder()
        .id(uuid::Uuid::now_v7())
        .name(format!("Reconcile sender {sender_code}"))
        .code(sender_code)
        .build()?;
    let sender = cala.accounts().create(sender).await?;

    let mut members = Vec::with_capacity(2);
    for i in 0..2 {
        let code = Alphanumeric.sample_string(&mut rand::rng(), 32);
        let acc = NewAccount::builder()
            .id(uuid::Uuid::now_v7())
            .name(format!("Reconcile member {i} {code}"))
            .code(code)
            .build()?;
        members.push(cala.accounts().create(acc).await?);
    }

    // amount + effective params, USD settled defaults — supports both
    // dated and undated posts.
    let tx_code = Alphanumeric.sample_string(&mut rand::rng(), 32);
    cala.tx_templates()
        .create(helpers::velocity_template(&tx_code))
        .await?;

    let journal_id = journal.id();
    Ok((
        Fixture {
            cala,
            journal_id,
            sender,
            members,
            tx_code,
        },
        jobs,
    ))
}

async fn create_ec_set(
    cala: &CalaLedger,
    journal_id: JournalId,
    name: &str,
) -> anyhow::Result<AccountSet> {
    let set = NewAccountSet::builder()
        .id(AccountSetId::new())
        .name(name)
        .journal_id(journal_id)
        .balance_rollup(BalanceRollup::EventuallyConsistent)
        .build()?;
    Ok(cala.account_sets().create(set).await?)
}

async fn create_ec_plain_account(cala: &CalaLedger, name: &str) -> anyhow::Result<Account> {
    let code = Alphanumeric.sample_string(&mut rand::rng(), 32);
    let account = NewAccount::builder()
        .id(uuid::Uuid::now_v7())
        .name(format!("{name} {code}"))
        .code(code)
        .balance_rollup(BalanceRollup::EventuallyConsistent)
        .build()?;
    Ok(cala.accounts().create(account).await?)
}

async fn post(
    fixture: &Fixture,
    recipient: AccountId,
    effective: Option<chrono::NaiveDate>,
) -> anyhow::Result<()> {
    let mut params = Params::new();
    params.insert("journal_id", fixture.journal_id.to_string());
    params.insert("sender", fixture.sender.id());
    params.insert("recipient", recipient);
    params.insert("amount", POST_AMOUNT);
    if let Some(date) = effective {
        params.insert("effective", date);
    }
    fixture
        .cala
        .post_transaction(TransactionId::new(), &fixture.tx_code, params)
        .await?;
    Ok(())
}

async fn post_round_robin(fixture: &Fixture, n_posts: usize) -> anyhow::Result<()> {
    for i in 0..n_posts {
        post(
            fixture,
            fixture.members[i % fixture.members.len()].id(),
            None,
        )
        .await?;
    }
    Ok(())
}

/// Clean state: verification reports zero drift and repair writes nothing.
#[tokio::test]
async fn verify_reports_no_drift_when_clean() -> anyhow::Result<()> {
    let usd: Currency = "USD".parse().unwrap();
    let pool = helpers::init_isolated_pool().await?;
    let (fixture, mut jobs) = setup(pool, helpers::test_journal()).await?;

    let ec_set = create_ec_set(&fixture.cala, fixture.journal_id, "clean EC set").await?;
    for m in &fixture.members {
        fixture
            .cala
            .account_sets()
            .add_member(ec_set.id(), m.id())
            .await?;
    }
    let ec_leaf = create_ec_plain_account(&fixture.cala, "clean EC leaf").await?;

    let n_posts = 6;
    post_round_robin(&fixture, n_posts).await?;
    post(&fixture, ec_leaf.id(), None).await?;
    let set_expected = POST_AMOUNT * Decimal::from(n_posts);

    jobs.start_poll().await?;
    helpers::wait_for_settled(
        &fixture.cala,
        fixture.journal_id,
        ec_set.id(),
        usd,
        set_expected,
    )
    .await?;
    helpers::wait_for_settled(
        &fixture.cala,
        fixture.journal_id,
        ec_leaf.id(),
        usd,
        POST_AMOUNT,
    )
    .await?;

    let targets = [AccountId::from(ec_set.id()), ec_leaf.id()];
    let reports = fixture
        .cala
        .balances()
        .verify_ec(fixture.journal_id, &targets)
        .await?;
    assert_eq!(reports.len(), 2);
    assert!(reports.iter().all(|r| !r.is_drifted()));
    assert!(reports
        .iter()
        .any(|r| r.account_id == AccountId::from(ec_set.id())
            && r.expected_version == n_posts as u32));

    let reports = fixture
        .cala
        .balances()
        .repair_ec(fixture.journal_id, &targets)
        .await?;
    assert!(reports.iter().all(|r| !r.repaired));

    // Repair-noop must not have changed the balances.
    let bal = fixture
        .cala
        .balances()
        .find(fixture.journal_id, ec_set.id(), usd)
        .await?;
    assert_eq!(bal.settled(), set_expected);
    Ok(())
}

/// A non-EC account is rejected — synchronous balances are maintained
/// inline and out of scope for the reconciler.
#[tokio::test]
async fn rejects_non_ec_targets() -> anyhow::Result<()> {
    let pool = helpers::init_isolated_pool().await?;
    let (fixture, _jobs) = setup(pool, helpers::test_journal()).await?;

    let result = fixture
        .cala
        .balances()
        .verify_ec(fixture.journal_id, &[fixture.sender.id()])
        .await;
    assert!(matches!(
        result,
        Err(BalanceError::NotEventuallyConsistent(id)) if id == fixture.sender.id()
    ));
    Ok(())
}

/// Corrupt `cala_current_balances.latest_values` → verify reports the
/// exact drift → repair restores the expected value with an appended
/// (version + 1) corrective snapshot → a second verify is clean.
#[tokio::test]
async fn detects_and_repairs_corrupted_current_balance() -> anyhow::Result<()> {
    let usd: Currency = "USD".parse().unwrap();
    let pool = helpers::init_isolated_pool().await?;
    let (fixture, mut jobs) = setup(pool.clone(), helpers::test_journal()).await?;

    let ec_set = create_ec_set(&fixture.cala, fixture.journal_id, "corrupted EC set").await?;
    for m in &fixture.members {
        fixture
            .cala
            .account_sets()
            .add_member(ec_set.id(), m.id())
            .await?;
    }

    let n_posts = 8;
    post_round_robin(&fixture, n_posts).await?;
    let expected = POST_AMOUNT * Decimal::from(n_posts);

    jobs.start_poll().await?;
    helpers::wait_for_settled(
        &fixture.cala,
        fixture.journal_id,
        ec_set.id(),
        usd,
        expected,
    )
    .await?;

    // Corrupt the settled credit balance (Decimal serializes as a JSON
    // string in the snapshot).
    let bogus = dec!(999999);
    sqlx::query(
        r#"
        UPDATE cala_current_balances
        SET latest_values =
            jsonb_set(latest_values, '{settled,cr_balance}', to_jsonb($3::text))
        WHERE journal_id = $1 AND account_id = $2
        "#,
    )
    .bind(uuid::Uuid::from(fixture.journal_id))
    .bind(uuid::Uuid::from(AccountId::from(ec_set.id())))
    .bind(bogus.to_string())
    .execute(&pool)
    .await?;

    let targets = [AccountId::from(ec_set.id())];
    let reports = fixture
        .cala
        .balances()
        .verify_ec(fixture.journal_id, &targets)
        .await?;
    assert_eq!(reports.len(), 1);
    let report = &reports[0];
    assert!(report.is_drifted());
    assert_eq!(report.settled.cr_delta, expected - bogus);
    assert_eq!(report.settled.dr_delta, Decimal::ZERO);
    assert_eq!(report.expected_version, n_posts as u32);
    assert_eq!(report.found_version, Some(n_posts as u32));

    let reports = fixture
        .cala
        .balances()
        .repair_ec(fixture.journal_id, &targets)
        .await?;
    assert!(reports[0].repaired);

    let bal = fixture
        .cala
        .balances()
        .find(fixture.journal_id, ec_set.id(), usd)
        .await?;
    assert_eq!(bal.settled(), expected);
    // Corrective snapshot appended on top of the existing chain.
    assert_eq!(bal.details.version, n_posts as u32 + 1);

    let reports = fixture
        .cala
        .balances()
        .verify_ec(fixture.journal_id, &targets)
        .await?;
    assert!(reports.iter().all(|r| !r.is_drifted()));
    Ok(())
}

/// Balance row and history wiped entirely → repair recreates the balance
/// from entries with the applier-identical end state.
#[tokio::test]
async fn repairs_deleted_balance_rows() -> anyhow::Result<()> {
    let usd: Currency = "USD".parse().unwrap();
    let pool = helpers::init_isolated_pool().await?;
    let (fixture, mut jobs) = setup(pool.clone(), helpers::test_journal()).await?;

    let ec_set = create_ec_set(&fixture.cala, fixture.journal_id, "wiped EC set").await?;
    for m in &fixture.members {
        fixture
            .cala
            .account_sets()
            .add_member(ec_set.id(), m.id())
            .await?;
    }

    let n_posts = 6;
    post_round_robin(&fixture, n_posts).await?;
    let expected = POST_AMOUNT * Decimal::from(n_posts);

    jobs.start_poll().await?;
    helpers::wait_for_settled(
        &fixture.cala,
        fixture.journal_id,
        ec_set.id(),
        usd,
        expected,
    )
    .await?;

    let journal_uuid = uuid::Uuid::from(fixture.journal_id);
    let set_uuid = uuid::Uuid::from(AccountId::from(ec_set.id()));
    // History references current — wipe it first.
    sqlx::query("DELETE FROM cala_balance_history WHERE journal_id = $1 AND account_id = $2")
        .bind(journal_uuid)
        .bind(set_uuid)
        .execute(&pool)
        .await?;
    sqlx::query("DELETE FROM cala_current_balances WHERE journal_id = $1 AND account_id = $2")
        .bind(journal_uuid)
        .bind(set_uuid)
        .execute(&pool)
        .await?;

    let targets = [AccountId::from(ec_set.id())];
    let reports = fixture
        .cala
        .balances()
        .verify_ec(fixture.journal_id, &targets)
        .await?;
    assert_eq!(reports.len(), 1);
    assert!(reports[0].is_drifted());
    assert_eq!(reports[0].found_version, None);
    assert_eq!(reports[0].settled.cr_delta, expected);

    fixture
        .cala
        .balances()
        .repair_ec(fixture.journal_id, &targets)
        .await?;

    let bal = fixture
        .cala
        .balances()
        .find(fixture.journal_id, ec_set.id(), usd)
        .await?;
    assert_eq!(bal.settled(), expected);
    // Fresh chain: the single corrective row carries the applier-identical
    // end version (one bump per folded entry).
    assert_eq!(bal.details.version, n_posts as u32);

    let reports = fixture
        .cala
        .balances()
        .verify_ec(fixture.journal_id, &targets)
        .await?;
    assert!(reports.iter().all(|r| !r.is_drifted()));
    Ok(())
}

/// The double-count guard: with the rollup job never run (cursor = 0),
/// posted-but-unapplied entries must contribute *nothing* to the expected
/// state — a naive Σ-entries rebuild would report (and "repair" in) a
/// balance the applier would then add again.
#[tokio::test]
async fn cursor_zero_expects_empty_state() -> anyhow::Result<()> {
    let usd: Currency = "USD".parse().unwrap();
    let pool = helpers::init_isolated_pool().await?;
    let (fixture, mut jobs) = setup(pool, helpers::test_journal()).await?;

    let ec_set = create_ec_set(&fixture.cala, fixture.journal_id, "cursor-zero EC set").await?;
    for m in &fixture.members {
        fixture
            .cala
            .account_sets()
            .add_member(ec_set.id(), m.id())
            .await?;
    }

    let n_posts = 6;
    post_round_robin(&fixture, n_posts).await?;
    let expected = POST_AMOUNT * Decimal::from(n_posts);

    // Job not polling: entries exist, nothing is applied, cursor is 0.
    let targets = [AccountId::from(ec_set.id())];
    let reports = fixture
        .cala
        .balances()
        .verify_ec(fixture.journal_id, &targets)
        .await?;
    assert!(
        reports.is_empty(),
        "no expected state and no found state ⇒ no involved pairs, got {reports:?}"
    );

    // Repair is likewise a no-op — and must not fabricate a balance the
    // applier would double-count later.
    let reports = fixture
        .cala
        .balances()
        .repair_ec(fixture.journal_id, &targets)
        .await?;
    assert!(reports.is_empty());

    // Now let the stream catch up: the full backlog folds exactly once on
    // top of the untouched state.
    jobs.start_poll().await?;
    helpers::wait_for_settled(
        &fixture.cala,
        fixture.journal_id,
        ec_set.id(),
        usd,
        expected,
    )
    .await?;
    let reports = fixture
        .cala
        .balances()
        .verify_ec(fixture.journal_id, &targets)
        .await?;
    assert_eq!(reports.len(), 1);
    assert!(!reports[0].is_drifted());
    Ok(())
}

/// The reconciler's exclusive advisory locks must block the streaming
/// applier *before* it writes or advances its cursor: hold the lock in a
/// raw transaction, observe the applier queued behind it (`pg_locks`),
/// release, and assert the backlog then folds exactly once.
#[tokio::test]
async fn exclusive_lock_blocks_applier_until_release() -> anyhow::Result<()> {
    let usd: Currency = "USD".parse().unwrap();
    let pool = helpers::init_isolated_pool().await?;
    let (fixture, mut jobs) = setup(pool.clone(), helpers::test_journal()).await?;

    let ec_set = create_ec_set(&fixture.cala, fixture.journal_id, "lock-race EC set").await?;
    for m in &fixture.members {
        fixture
            .cala
            .account_sets()
            .add_member(ec_set.id(), m.id())
            .await?;
    }

    let n_posts = 6;
    post_round_robin(&fixture, n_posts).await?;
    let expected = POST_AMOUNT * Decimal::from(n_posts);

    // Take the reconciler's exclusive lock on the EC set (same class-1
    // key the tool uses) in a dedicated transaction.
    let mut lock_tx = pool.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock(1, hashtext($1::uuid::text))")
        .bind(uuid::Uuid::from(AccountId::from(ec_set.id())))
        .execute(&mut *lock_tx)
        .await?;

    jobs.start_poll().await?;

    // The applier must queue behind the exclusive lock: an ungranted
    // advisory waiter appears and the EC set stays without a balance.
    let mut saw_waiter = false;
    for _ in 0..300 {
        let waiters: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM pg_locks WHERE locktype = 'advisory' AND NOT granted",
        )
        .fetch_one(&pool)
        .await?;
        if waiters > 0 {
            saw_waiter = true;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    assert!(saw_waiter, "applier never queued behind the exclusive lock");
    assert!(
        fixture
            .cala
            .balances()
            .find(fixture.journal_id, ec_set.id(), usd)
            .await
            .is_err(),
        "EC set must not gain a balance while the exclusive lock is held",
    );

    // Release: the blocked batch applies on top, exactly once.
    lock_tx.rollback().await?;
    helpers::wait_for_settled(
        &fixture.cala,
        fixture.journal_id,
        ec_set.id(),
        usd,
        expected,
    )
    .await?;

    let reports = fixture
        .cala
        .balances()
        .verify_ec(fixture.journal_id, &[AccountId::from(ec_set.id())])
        .await?;
    assert!(reports.iter().all(|r| !r.is_drifted()));
    Ok(())
}

/// Effective-series repair for a back-dated history: corrupt the series
/// (drop an interior date, corrupt the latest) → verify flags effective
/// drift with the settled state clean → repair rebuilds the whole
/// per-date cumulative series.
#[tokio::test]
async fn repairs_backdated_effective_series() -> anyhow::Result<()> {
    let usd: Currency = "USD".parse().unwrap();
    let pool = helpers::init_isolated_pool().await?;
    let (fixture, mut jobs) = setup(
        pool.clone(),
        helpers::test_journal_with_effective_balances(),
    )
    .await?;

    let ec_set = create_ec_set(&fixture.cala, fixture.journal_id, "effective EC set").await?;
    let member = fixture.members[0].id();
    fixture
        .cala
        .account_sets()
        .add_member(ec_set.id(), member)
        .await?;

    let today = chrono::Utc::now().date_naive();
    let d1 = today - chrono::Days::new(2);
    let d2 = today - chrono::Days::new(1);

    // Post d2 first, then back-date d1 — exercising the applier's
    // rewrite path before any corruption.
    post(&fixture, member, Some(d2)).await?;
    post(&fixture, member, Some(d1)).await?;

    jobs.start_poll().await?;
    helpers::wait_for_effective(
        &fixture.cala,
        fixture.journal_id,
        ec_set.id(),
        usd,
        d1,
        POST_AMOUNT,
    )
    .await?;
    helpers::wait_for_effective(
        &fixture.cala,
        fixture.journal_id,
        ec_set.id(),
        usd,
        d2,
        POST_AMOUNT * dec!(2),
    )
    .await?;

    let journal_uuid = uuid::Uuid::from(fixture.journal_id);
    let set_uuid = uuid::Uuid::from(AccountId::from(ec_set.id()));
    // Drop the interior date and corrupt the latest cumulative row.
    sqlx::query(
        "DELETE FROM cala_cumulative_effective_balances
         WHERE journal_id = $1 AND account_id = $2 AND effective = $3",
    )
    .bind(journal_uuid)
    .bind(set_uuid)
    .bind(d1)
    .execute(&pool)
    .await?;
    sqlx::query(
        r#"
        UPDATE cala_cumulative_effective_balances
        SET values = jsonb_set(values, '{settled,cr_balance}', to_jsonb($3::text))
        WHERE journal_id = $1 AND account_id = $2 AND effective = $4
        "#,
    )
    .bind(journal_uuid)
    .bind(set_uuid)
    .bind(dec!(424242).to_string())
    .bind(d2)
    .execute(&pool)
    .await?;

    let targets = [AccountId::from(ec_set.id())];
    let reports = fixture
        .cala
        .balances()
        .verify_ec(fixture.journal_id, &targets)
        .await?;
    assert_eq!(reports.len(), 1);
    assert!(reports[0].effective_drift);
    assert!(!reports[0].balance_drift());

    fixture
        .cala
        .balances()
        .repair_ec(fixture.journal_id, &targets)
        .await?;

    // Whole series restored — including the dropped interior date.
    let bal = fixture
        .cala
        .balances()
        .effective()
        .find_cumulative(fixture.journal_id, ec_set.id(), usd, d1)
        .await?;
    assert_eq!(bal.settled(), POST_AMOUNT);
    let bal = fixture
        .cala
        .balances()
        .effective()
        .find_cumulative(fixture.journal_id, ec_set.id(), usd, d2)
        .await?;
    assert_eq!(bal.settled(), POST_AMOUNT * dec!(2));

    let reports = fixture
        .cala
        .balances()
        .verify_ec(fixture.journal_id, &targets)
        .await?;
    assert!(reports.iter().all(|r| !r.is_drifted()));
    Ok(())
}

/// After a repair the stream keeps flowing: later postings fold on top of
/// the corrected state exactly once.
#[tokio::test]
async fn streaming_continues_after_repair() -> anyhow::Result<()> {
    let usd: Currency = "USD".parse().unwrap();
    let pool = helpers::init_isolated_pool().await?;
    let (fixture, mut jobs) = setup(pool.clone(), helpers::test_journal()).await?;

    let ec_set = create_ec_set(&fixture.cala, fixture.journal_id, "repair-then-stream").await?;
    for m in &fixture.members {
        fixture
            .cala
            .account_sets()
            .add_member(ec_set.id(), m.id())
            .await?;
    }

    let n_initial = 6;
    post_round_robin(&fixture, n_initial).await?;
    let initial = POST_AMOUNT * Decimal::from(n_initial);

    jobs.start_poll().await?;
    helpers::wait_for_settled(&fixture.cala, fixture.journal_id, ec_set.id(), usd, initial).await?;

    sqlx::query(
        r#"
        UPDATE cala_current_balances
        SET latest_values =
            jsonb_set(latest_values, '{settled,cr_balance}', to_jsonb($3::text))
        WHERE journal_id = $1 AND account_id = $2
        "#,
    )
    .bind(uuid::Uuid::from(fixture.journal_id))
    .bind(uuid::Uuid::from(AccountId::from(ec_set.id())))
    .bind(dec!(1).to_string())
    .execute(&pool)
    .await?;

    let targets = [AccountId::from(ec_set.id())];
    let reports = fixture
        .cala
        .balances()
        .repair_ec(fixture.journal_id, &targets)
        .await?;
    assert!(reports[0].repaired);

    // New activity lands on the corrected state, exactly once.
    let n_more = 4;
    post_round_robin(&fixture, n_more).await?;
    let total = initial + POST_AMOUNT * Decimal::from(n_more);
    helpers::wait_for_settled(&fixture.cala, fixture.journal_id, ec_set.id(), usd, total).await?;

    let reports = fixture
        .cala
        .balances()
        .verify_ec(fixture.journal_id, &targets)
        .await?;
    assert!(reports.iter().all(|r| !r.is_drifted()));
    Ok(())
}
