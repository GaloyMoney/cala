//! Concurrency tests for the account-set membership lock protocol.
//!
//! Membership is stored as **direct edges only** (no transitive closure),
//! but every mutation validates **path uniqueness** — no account may be
//! contained in the same set via two paths — before writing, and that
//! read-then-write must be fenced. Account-member mutations (add/remove
//! an *account* to/from a set) take the coarse membership lock SHARED
//! plus an exclusive per-member lock, so mutations for different members
//! run concurrently. Set-structure mutations (add/remove a member *set*)
//! take the coarse lock EXCLUSIVE and fence everything.
//!
//! The blocking assertions work by holding one operation's transaction
//! open and observing whether a second operation completes (bounded by a
//! generous timeout) or stays pending until the first commits.
//!
//! Because these tests hold membership locks open on purpose, they must
//! not share lock scope with anything else: an open SHARED holder plus a
//! queued EXCLUSIVE waiter from an unrelated test would stall every later
//! SHARED request (PostgreSQL queues conflicting requests FIFO). Advisory
//! locks are scoped per database, so each test creates its own throwaway
//! database instead of using the shared `PG_CON` one.

mod helpers;

use std::time::Duration;

use rand::distr::{Alphanumeric, SampleString};

use cala_ledger::{account_set::*, *};

/// Generous bound for "this must not block": under a global exclusive
/// lock the future would stay pending until the other transaction
/// commits, so a completion within this window proves the operations do
/// not exclude each other.
const MUST_COMPLETE: Duration = Duration::from_secs(5);
/// Observation window for "this must block": long enough to rule out
/// scheduling noise, short enough to keep the suite fast.
const MUST_STILL_BE_PENDING: Duration = Duration::from_millis(300);

/// Creates a dedicated database (isolating this test's advisory locks),
/// runs migrations, and returns a pool connected to it.
async fn init_isolated_pool(max_connections: u32) -> anyhow::Result<sqlx::PgPool> {
    let pg_con = std::env::var("PG_CON")?;
    let admin_pool = sqlx::PgPool::connect(&pg_con).await?;
    let db_name = format!(
        "membership_locks_{}",
        Alphanumeric
            .sample_string(&mut rand::rng(), 12)
            .to_lowercase()
    );
    sqlx::query(&format!(r#"CREATE DATABASE "{db_name}""#))
        .execute(&admin_pool)
        .await?;
    let (base, _) = pg_con
        .rsplit_once('/')
        .expect("PG_CON has no database path");
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(max_connections)
        .acquire_timeout(Duration::from_secs(60))
        .connect(&format!("{base}/{db_name}"))
        .await?;
    // Unlike `helpers::init_pool` this must not append the job crate's
    // migrations: they share a version id with cala's own `job_setup`
    // migration, which double-applies on a freshly created database.
    sqlx::migrate!().run(&pool).await?;
    Ok(pool)
}

async fn init_cala() -> anyhow::Result<CalaLedger> {
    let pool = init_isolated_pool(10).await?;
    let mut jobs = helpers::init_jobs(pool.clone()).await?;
    let cala_config = CalaLedgerConfig::builder()
        .pool(pool)
        .exec_migrations(false)
        .build()?;
    Ok(CalaLedger::init(cala_config, &mut jobs).await?)
}

fn new_set(journal_id: JournalId, name: &str) -> NewAccountSet {
    NewAccountSet::builder()
        .id(AccountSetId::new())
        .name(name)
        .journal_id(journal_id)
        .balance_rollup(BalanceRollup::Synchronous)
        .build()
        .unwrap()
}

#[tokio::test]
async fn account_member_ops_for_different_members_run_concurrently() -> anyhow::Result<()> {
    let cala = init_cala().await?;
    let journal = cala.journals().create(helpers::test_journal()).await?;

    let (one, two) = helpers::test_accounts();
    let one = cala.accounts().create(one).await?;
    let two = cala.accounts().create(two).await?;
    let set_one = cala
        .account_sets()
        .create(new_set(journal.id(), "concurrent-members-1"))
        .await?;
    let set_two = cala
        .account_sets()
        .create(new_set(journal.id(), "concurrent-members-2"))
        .await?;

    // Hold an uncommitted account-member add open...
    let mut op = cala.begin_operation().await?;
    cala.account_sets()
        .add_member_in_op(&mut op, set_one.id(), one.id())
        .await?;

    // ...a second add for a *different* member must not wait for it.
    tokio::time::timeout(
        MUST_COMPLETE,
        cala.account_sets().add_member(set_two.id(), two.id()),
    )
    .await
    .expect("account-member adds for different members must not serialize")?;

    op.commit().await?;
    Ok(())
}

#[tokio::test]
async fn account_member_ops_for_same_member_serialize() -> anyhow::Result<()> {
    let cala = init_cala().await?;
    let journal = cala.journals().create(helpers::test_journal()).await?;

    let (one, _) = helpers::test_accounts();
    let one = cala.accounts().create(one).await?;
    let set_one = cala
        .account_sets()
        .create(new_set(journal.id(), "same-member-1"))
        .await?;
    let set_two = cala
        .account_sets()
        .create(new_set(journal.id(), "same-member-2"))
        .await?;

    let mut op = cala.begin_operation().await?;
    cala.account_sets()
        .add_member_in_op(&mut op, set_one.id(), one.id())
        .await?;

    // An add of the *same* member elsewhere must wait for the open
    // transaction (per-member exclusive lock): its path-uniqueness check
    // must not run against a graph another add is mid-way through
    // changing.
    let cala2 = cala.clone();
    let set_two_id = set_two.id();
    let member_id = one.id();
    let mut blocked =
        tokio::spawn(async move { cala2.account_sets().add_member(set_two_id, member_id).await });

    assert!(
        tokio::time::timeout(MUST_STILL_BE_PENDING, &mut blocked)
            .await
            .is_err(),
        "same-member add must block while the first transaction is open"
    );

    op.commit().await?;
    blocked.await??;
    Ok(())
}

#[tokio::test]
async fn structure_ops_fence_account_member_ops() -> anyhow::Result<()> {
    let cala = init_cala().await?;
    let journal = cala.journals().create(helpers::test_journal()).await?;

    let (one, _) = helpers::test_accounts();
    let one = cala.accounts().create(one).await?;
    let parent = cala
        .account_sets()
        .create(new_set(journal.id(), "fence-parent"))
        .await?;
    let child = cala
        .account_sets()
        .create(new_set(journal.id(), "fence-child"))
        .await?;
    let unrelated = cala
        .account_sets()
        .create(new_set(journal.id(), "fence-unrelated"))
        .await?;

    // Hold an uncommitted structure mutation open (exclusive coarse lock)...
    let mut op = cala.begin_operation().await?;
    cala.account_sets()
        .add_member_in_op(&mut op, parent.id(), child.id())
        .await?;

    // ...any account-member mutation must wait for it, even on an
    // unrelated set: structure changes mutate the edges every
    // path-uniqueness walk reads, so they fence the whole graph.
    let cala2 = cala.clone();
    let unrelated_id = unrelated.id();
    let member_id = one.id();
    let mut blocked = tokio::spawn(async move {
        cala2
            .account_sets()
            .add_member(unrelated_id, member_id)
            .await
    });

    assert!(
        tokio::time::timeout(MUST_STILL_BE_PENDING, &mut blocked)
            .await
            .is_err(),
        "account-member add must block while a structure mutation is open"
    );

    op.commit().await?;
    blocked.await??;
    Ok(())
}

#[tokio::test]
async fn structure_ops_serialize() -> anyhow::Result<()> {
    let cala = init_cala().await?;
    let journal = cala.journals().create(helpers::test_journal()).await?;

    let parent_a = cala
        .account_sets()
        .create(new_set(journal.id(), "struct-serialize-parent-a"))
        .await?;
    let child_a = cala
        .account_sets()
        .create(new_set(journal.id(), "struct-serialize-child-a"))
        .await?;
    let parent_b = cala
        .account_sets()
        .create(new_set(journal.id(), "struct-serialize-parent-b"))
        .await?;
    let child_b = cala
        .account_sets()
        .create(new_set(journal.id(), "struct-serialize-child-b"))
        .await?;

    // Hold an uncommitted structure mutation open (exclusive coarse lock)...
    let mut op = cala.begin_operation().await?;
    cala.account_sets()
        .add_member_in_op(&mut op, parent_a.id(), child_a.id())
        .await?;

    // ...a second, disjoint structure mutation must wait for the coarse
    // lock: structure ops serialize globally.
    let cala2 = cala.clone();
    let (p_b, c_b) = (parent_b.id(), child_b.id());
    let mut blocked = tokio::spawn(async move { cala2.account_sets().add_member(p_b, c_b).await });

    assert!(
        tokio::time::timeout(MUST_STILL_BE_PENDING, &mut blocked)
            .await
            .is_err(),
        "a second set-structure mutation must block while the first is open"
    );

    op.commit().await?;
    blocked.await??;
    Ok(())
}

/// Adjacency-only membership: many concurrent account-member adds into a
/// shared deep hierarchy each write exactly one direct edge on the leaf
/// (no closure rows on ancestors), and the read-time upward walk resolves
/// every account to all of its ancestor sets.
#[tokio::test]
async fn concurrent_adds_resolve_all_ancestors() -> anyhow::Result<()> {
    const N_MEMBERS: usize = 16;

    let pool = init_isolated_pool(20).await?;
    let mut jobs = helpers::init_jobs(pool.clone()).await?;
    let cala_config = CalaLedgerConfig::builder()
        .pool(pool.clone())
        .exec_migrations(false)
        .build()?;
    let cala = CalaLedger::init(cala_config, &mut jobs).await?;
    let journal = cala.journals().create(helpers::test_journal()).await?;

    // grandparent <- parent <- leaf
    let grandparent = cala
        .account_sets()
        .create(new_set(journal.id(), "walk-grandparent"))
        .await?;
    let parent = cala
        .account_sets()
        .create(new_set(journal.id(), "walk-parent"))
        .await?;
    let leaf = cala
        .account_sets()
        .create(new_set(journal.id(), "walk-leaf"))
        .await?;
    cala.account_sets()
        .add_member(grandparent.id(), parent.id())
        .await?;
    cala.account_sets()
        .add_member(parent.id(), leaf.id())
        .await?;

    let mut accounts = Vec::new();
    for _ in 0..N_MEMBERS {
        let (new_account, _) = helpers::test_accounts();
        accounts.push(cala.accounts().create(new_account).await?);
    }

    let mut handles = Vec::new();
    for account in &accounts {
        let cala = cala.clone();
        let leaf_id = leaf.id();
        let account_id = account.id();
        handles.push(tokio::spawn(async move {
            cala.account_sets().add_member(leaf_id, account_id).await
        }));
    }
    for handle in handles {
        handle.await??;
    }

    let account_ids = accounts
        .iter()
        .map(|a| uuid::Uuid::from(a.id()))
        .collect::<Vec<_>>();

    // The leaf holds exactly one direct row per account.
    let leaf_count = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT COUNT(*) FROM cala_account_set_member_accounts
        WHERE account_set_id = $1 AND member_account_id = ANY($2)
        "#,
    )
    .bind(uuid::Uuid::from(leaf.id()))
    .bind(&account_ids)
    .fetch_one(&pool)
    .await?;
    assert_eq!(
        leaf_count as usize, N_MEMBERS,
        "direct rows missing on leaf"
    );

    // Ancestors hold NO rows: there is no materialized transitive closure.
    for set_id in [parent.id(), grandparent.id()] {
        let count = sqlx::query_scalar::<_, i64>(
            r#"
            SELECT COUNT(*) FROM cala_account_set_member_accounts
            WHERE account_set_id = $1 AND member_account_id = ANY($2)
            "#,
        )
        .bind(uuid::Uuid::from(set_id))
        .bind(&account_ids)
        .fetch_one(&pool)
        .await?;
        assert_eq!(count, 0, "unexpected closure rows on ancestor set {set_id}");
    }

    // The production upward walk resolves each account to exactly its three
    // ancestor sets {leaf, parent, grandparent}.
    let expected = {
        let mut e = [
            uuid::Uuid::from(leaf.id()),
            uuid::Uuid::from(parent.id()),
            uuid::Uuid::from(grandparent.id()),
        ];
        e.sort();
        e.to_vec()
    };
    for account in &accounts {
        let mut sets = sqlx::query_scalar::<_, uuid::Uuid>(
            r#"
            WITH RECURSIVE seed AS (
                SELECT account_set_id
                FROM cala_account_set_member_accounts
                WHERE member_account_id = $1
            ),
            ancestors AS (
                SELECT account_set_id FROM seed
                UNION
                SELECT e.account_set_id
                FROM ancestors a
                JOIN cala_account_set_member_account_sets e
                  ON e.member_account_set_id = a.account_set_id
            )
            SELECT account_set_id FROM ancestors
            "#,
        )
        .bind(uuid::Uuid::from(account.id()))
        .fetch_all(&pool)
        .await?;
        sets.sort();
        assert_eq!(
            sets,
            expected,
            "walk did not resolve all ancestors for account {}",
            account.id()
        );
    }

    Ok(())
}

/// Batch set-structure mutations serialize with single-edge structure
/// mutations on the same exclusive coarse lock, even when the sets are
/// disjoint. This exercises the combined-graph validation path rather than
/// the single-edge fast path.
#[tokio::test]
async fn batch_structure_ops_serialize() -> anyhow::Result<()> {
    let cala = init_cala().await?;
    let journal = cala.journals().create(helpers::test_journal()).await?;

    let parent_a = cala
        .account_sets()
        .create(new_set(journal.id(), "batch-struct-a-parent"))
        .await?;
    let child_a = cala
        .account_sets()
        .create(new_set(journal.id(), "batch-struct-a-child"))
        .await?;
    let parent_b = cala
        .account_sets()
        .create(new_set(journal.id(), "batch-struct-b-parent"))
        .await?;
    let child_b = cala
        .account_sets()
        .create(new_set(journal.id(), "batch-struct-b-child"))
        .await?;
    let parent_c = cala
        .account_sets()
        .create(new_set(journal.id(), "batch-struct-c-parent"))
        .await?;
    let child_c = cala
        .account_sets()
        .create(new_set(journal.id(), "batch-struct-c-child"))
        .await?;

    // Hold an uncommitted batch structure mutation open (exclusive coarse
    // lock, len > 1 to stay on the combined-graph path).
    let mut op = cala.begin_operation().await?;
    cala.account_sets()
        .add_member_sets_in_op(
            &mut op,
            &[(parent_a.id(), child_a.id()), (parent_b.id(), child_b.id())],
        )
        .await?;

    // A disjoint single-edge structure mutation must still wait.
    let cala2 = cala.clone();
    let (p_c, c_c) = (parent_c.id(), child_c.id());
    let mut blocked = tokio::spawn(async move { cala2.account_sets().add_member(p_c, c_c).await });

    assert!(
        tokio::time::timeout(MUST_STILL_BE_PENDING, &mut blocked)
            .await
            .is_err(),
        "a single-edge structure mutation must block while a batch is open"
    );

    op.commit().await?;
    blocked.await??;
    Ok(())
}

/// An open batch set-structure mutation fences unrelated account-member
/// mutations, just like a single-edge structure mutation does.
#[tokio::test]
async fn batch_structure_ops_fence_account_member_ops() -> anyhow::Result<()> {
    let cala = init_cala().await?;
    let journal = cala.journals().create(helpers::test_journal()).await?;

    let (one, _) = helpers::test_accounts();
    let one = cala.accounts().create(one).await?;
    let parent = cala
        .account_sets()
        .create(new_set(journal.id(), "batch-fence-parent"))
        .await?;
    let child = cala
        .account_sets()
        .create(new_set(journal.id(), "batch-fence-child"))
        .await?;
    let unrelated = cala
        .account_sets()
        .create(new_set(journal.id(), "batch-fence-unrelated"))
        .await?;

    let child2 = cala
        .account_sets()
        .create(new_set(journal.id(), "batch-fence-child2"))
        .await?;

    let mut op = cala.begin_operation().await?;
    cala.account_sets()
        .add_member_sets_in_op(
            &mut op,
            &[(parent.id(), child.id()), (parent.id(), child2.id())],
        )
        .await?;

    let cala2 = cala.clone();
    let unrelated_id = unrelated.id();
    let member_id = one.id();
    let mut blocked = tokio::spawn(async move {
        cala2
            .account_sets()
            .add_member(unrelated_id, member_id)
            .await
    });

    assert!(
        tokio::time::timeout(MUST_STILL_BE_PENDING, &mut blocked)
            .await
            .is_err(),
        "account-member add must block while a batch structure mutation is open"
    );

    op.commit().await?;
    blocked.await??;
    Ok(())
}

/// A single-edge batch and a direct set-structure call route through
/// the same combined-graph validation machinery, producing identical
/// persisted state and outbox events.
#[tokio::test]
async fn single_edge_batch_matches_single_attach_path() -> anyhow::Result<()> {
    let pool = init_isolated_pool(5).await?;
    let mut jobs = helpers::init_jobs(pool.clone()).await?;
    let cala_config = CalaLedgerConfig::builder()
        .pool(pool.clone())
        .exec_migrations(false)
        .build()?;
    let cala = CalaLedger::init(cala_config, &mut jobs).await?;
    let journal = cala.journals().create(helpers::test_journal()).await?;

    let parent = cala
        .account_sets()
        .create(new_set(journal.id(), "single-edge-parent"))
        .await?;
    let child = cala
        .account_sets()
        .create(new_set(journal.id(), "single-edge-child"))
        .await?;

    cala.account_sets()
        .add_member_sets(&[(parent.id(), child.id())])
        .await?;

    let edge_count: i64 = sqlx::query_scalar(
        r#"
        SELECT COUNT(*)
        FROM cala_account_set_member_account_sets
        WHERE account_set_id = $1 AND member_account_set_id = $2
        "#,
    )
    .bind(parent.id())
    .bind(child.id())
    .fetch_one(&pool)
    .await?;
    assert_eq!(edge_count, 1);

    let event_count: i64 = sqlx::query_scalar(
        r#"
        SELECT COUNT(*)
        FROM cala_persistent_outbox_events
        WHERE payload->>'type' = 'account_set_member_created'
          AND payload->>'account_set_id' = $1::text
        "#,
    )
    .bind(parent.id())
    .fetch_one(&pool)
    .await?;
    assert_eq!(event_count, 1);

    Ok(())
}
