//! Tests for the lock-free create-inside-set fast path
//! (`NewAccount::initial_account_set`): a freshly created account joins
//! exactly one account set in the same atomic operation, taking ONLY the
//! class-2 per-member advisory lock — no coarse membership-graph lock,
//! no class-1 balance-history guard lock, no path-uniqueness walk. The
//! invariant argument lives on the `initial_account_set` field docs
//! (k=1 is enforced at the type level — an `Option`, not a collection).
//!
//! Style mirrors `account_set_membership_locks.rs`: blocking assertions
//! hold one operation's transaction open and observe whether a second
//! operation completes (bounded by a generous timeout) or stays pending
//! until the first commits; lock footprints are inspected via `pg_locks`
//! from a second connection. Because these tests hold advisory locks
//! open on purpose (and one drives the global EC rollup job), each test
//! runs on its own throwaway database.

mod helpers;

use std::time::Duration;

use rand::distr::{Alphanumeric, SampleString};
use rust_decimal_macros::dec;

use cala_ledger::{
    account::{error::AccountError, NewAccount},
    account_set::{error::AccountSetError, NewAccountSet},
    tx_template::Params,
    *,
};

/// Generous bound for "this must not block": under a global exclusive
/// lock the future would stay pending until the other transaction
/// commits, so a completion within this window proves the operations do
/// not exclude each other.
const MUST_COMPLETE: Duration = Duration::from_secs(5);
/// Observation window for "this must block": long enough to rule out
/// scheduling noise, short enough to keep the suite fast.
const MUST_STILL_BE_PENDING: Duration = Duration::from_millis(300);

async fn init_cala() -> anyhow::Result<(CalaLedger, job::Jobs, sqlx::PgPool)> {
    let pool = helpers::init_isolated_pool().await?;
    let mut jobs = helpers::init_jobs(pool.clone()).await?;
    let cala_config = CalaLedgerConfig::builder()
        .pool(pool.clone())
        .exec_migrations(false)
        .build()?;
    let cala = CalaLedger::init(cala_config, &mut jobs).await?;
    Ok((cala, jobs, pool))
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

fn new_ec_set(journal_id: JournalId, name: &str) -> NewAccountSet {
    NewAccountSet::builder()
        .id(AccountSetId::new())
        .name(name)
        .journal_id(journal_id)
        .balance_rollup(BalanceRollup::EventuallyConsistent)
        .build()
        .unwrap()
}

fn new_account_in(set_id: AccountSetId) -> NewAccount {
    let code = Alphanumeric.sample_string(&mut rand::rng(), 32);
    NewAccount::builder()
        .id(uuid::Uuid::now_v7())
        .name(format!("Fast path member {code}"))
        .code(code)
        .initial_account_set(set_id)
        .build()
        .unwrap()
}

/// Count the direct membership rows for `(set, account)`.
async fn direct_membership_count(
    pool: &sqlx::PgPool,
    set_id: AccountSetId,
    account_id: AccountId,
) -> anyhow::Result<i64> {
    Ok(sqlx::query_scalar::<_, i64>(
        r#"
        SELECT COUNT(*) FROM cala_account_set_member_accounts
        WHERE account_set_id = $1 AND member_account_id = $2
        "#,
    )
    .bind(uuid::Uuid::from(set_id))
    .bind(uuid::Uuid::from(account_id))
    .fetch_one(pool)
    .await?)
}

/// Resolve `account_id`'s ancestor sets with the production-shaped upward
/// walk, counting membership *paths* per ancestor (UNION ALL preserves
/// path multiplicity — the path-uniqueness invariant demands exactly 1).
async fn ancestor_path_counts(
    pool: &sqlx::PgPool,
    account_id: AccountId,
) -> anyhow::Result<Vec<(uuid::Uuid, i64)>> {
    Ok(sqlx::query_as::<_, (uuid::Uuid, i64)>(
        r#"
        WITH RECURSIVE containments AS (
            SELECT account_set_id
            FROM cala_account_set_member_accounts
            WHERE member_account_id = $1
            UNION ALL
            SELECT e.account_set_id
            FROM containments c
            JOIN cala_account_set_member_account_sets e
              ON e.member_account_set_id = c.account_set_id
        )
        SELECT account_set_id, COUNT(*) FROM containments
        GROUP BY account_set_id ORDER BY account_set_id
        "#,
    )
    .bind(uuid::Uuid::from(account_id))
    .fetch_all(pool)
    .await?)
}

/// Fetch the `account_set_member_created` outbox payload for `(set,
/// account)` as canonical jsonb text.
async fn member_created_payload(
    pool: &sqlx::PgPool,
    set_id: AccountSetId,
) -> anyhow::Result<String> {
    Ok(sqlx::query_scalar::<_, String>(
        r#"
        SELECT payload::text FROM cala_persistent_outbox_events
        WHERE payload->>'type' = 'account_set_member_created'
          AND payload->>'account_set_id' = $1::text
        "#,
    )
    .bind(uuid::Uuid::from(set_id))
    .fetch_one(pool)
    .await?)
}

/// Batch happy path: three fresh accounts each join a different set in
/// one `create_all`; memberships exist, ancestor resolution sees them,
/// and the outbox event is byte-identical (modulo ids) to the one the
/// classic create-then-attach flow emits.
#[tokio::test]
async fn batch_create_attaches_and_matches_classic_events() -> anyhow::Result<()> {
    let (cala, _jobs, pool) = init_cala().await?;
    let journal = cala.journals().create(helpers::test_journal()).await?;

    let sets = [
        cala.account_sets()
            .create(new_set(journal.id(), "fast-batch-1"))
            .await?,
        cala.account_sets()
            .create(new_set(journal.id(), "fast-batch-2"))
            .await?,
        cala.account_sets()
            .create(new_set(journal.id(), "fast-batch-3"))
            .await?,
    ];
    let parent = cala
        .account_sets()
        .create(new_set(journal.id(), "fast-batch-parent"))
        .await?;
    cala.account_sets()
        .add_member(parent.id(), sets[0].id())
        .await?;

    let new_accounts: Vec<NewAccount> = sets.iter().map(|s| new_account_in(s.id())).collect();
    let accounts = cala.accounts().create_all(new_accounts).await?;

    for (set, account) in sets.iter().zip(accounts.iter()) {
        assert_eq!(
            direct_membership_count(&pool, set.id(), account.id()).await?,
            1
        );
    }

    let paths = ancestor_path_counts(&pool, accounts[0].id()).await?;
    let mut resolved: Vec<uuid::Uuid> = paths.iter().map(|(id, _)| *id).collect();
    resolved.sort();
    let mut expected = vec![
        uuid::Uuid::from(sets[0].id()),
        uuid::Uuid::from(parent.id()),
    ];
    expected.sort();
    assert_eq!(resolved, expected);
    assert!(paths.iter().all(|(_, n)| *n == 1));

    let classic_set = cala
        .account_sets()
        .create(new_set(journal.id(), "fast-batch-classic"))
        .await?;
    let (classic_account, _) = helpers::test_accounts();
    let classic_account = cala.accounts().create(classic_account).await?;
    cala.account_sets()
        .add_member(classic_set.id(), classic_account.id())
        .await?;

    let classic_payload = member_created_payload(&pool, classic_set.id()).await?;
    for (set, account) in sets.iter().zip(accounts.iter()) {
        let fast_payload = member_created_payload(&pool, set.id()).await?;
        let expected = classic_payload
            .replace(&classic_set.id().to_string(), &set.id().to_string())
            .replace(&classic_account.id().to_string(), &account.id().to_string());
        assert_eq!(
            fast_payload, expected,
            "fast-path member event must be byte-identical to the classic flow's"
        );
    }

    Ok(())
}

/// An unset `initial_account_set` is a plain create — no membership
/// rows, account fully usable.
#[tokio::test]
async fn empty_field_is_plain_create() -> anyhow::Result<()> {
    let (cala, _jobs, pool) = init_cala().await?;

    let (new_account, _) = helpers::test_accounts();
    let account = cala.accounts().create(new_account).await?;

    let rows = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM cala_account_set_member_accounts WHERE member_account_id = $1",
    )
    .bind(uuid::Uuid::from(account.id()))
    .fetch_one(&pool)
    .await?;
    assert_eq!(rows, 0);
    assert_eq!(cala.accounts().find(account.id()).await?.id(), account.id());
    Ok(())
}

/// Lock footprint: while a fast-path create is open (pre-commit) it
/// holds EXACTLY the class-2 per-member advisory lock for its account —
/// not the coarse membership-graph lock (key 123456, any form, any
/// mode) and no class-1 lock.
#[tokio::test]
async fn fast_path_lock_footprint() -> anyhow::Result<()> {
    let (cala, _jobs, pool) = init_cala().await?;
    let journal = cala.journals().create(helpers::test_journal()).await?;
    let set = cala
        .account_sets()
        .create(new_set(journal.id(), "fast-lock-footprint"))
        .await?;

    let new_account = new_account_in(set.id());
    let account_id = new_account.id;

    let mut op = cala.begin_operation().await?;
    cala.accounts().create_in_op(&mut op, new_account).await?;

    let advisory_locks = sqlx::query_as::<_, (i64, i64)>(
        r#"
        SELECT classid::bigint, objid::bigint FROM pg_locks
        WHERE locktype = 'advisory'
          AND database = (SELECT oid FROM pg_database WHERE datname = current_database())
        "#,
    )
    .fetch_all(&pool)
    .await?;

    let expected_objid = sqlx::query_scalar::<_, i64>("SELECT hashtext($1)::bigint & 4294967295")
        .bind(account_id.to_string())
        .fetch_one(&pool)
        .await?;

    assert_eq!(
        advisory_locks,
        vec![(2, expected_objid)],
        "fast path must hold exactly its class-2 per-member lock; \
         no coarse (123456) and no class-1 lock — got {advisory_locks:?}"
    );

    op.commit().await?;
    assert_eq!(
        direct_membership_count(&pool, set.id(), account_id).await?,
        1
    );
    Ok(())
}

/// No-convoy: while a structure mutation holds the coarse
/// membership-graph lock EXCLUSIVE (an open `add_member_set`), a
/// fast-path create COMPLETES — the classic attach would block here
/// (see `structure_ops_fence_account_member_ops` in
/// `account_set_membership_locks.rs`).
#[tokio::test]
async fn fast_path_completes_under_exclusive_structure_lock() -> anyhow::Result<()> {
    let (cala, _jobs, pool) = init_cala().await?;
    let journal = cala.journals().create(helpers::test_journal()).await?;

    let parent = cala
        .account_sets()
        .create(new_set(journal.id(), "no-convoy-parent"))
        .await?;
    let child = cala
        .account_sets()
        .create(new_set(journal.id(), "no-convoy-child"))
        .await?;
    let target = cala
        .account_sets()
        .create(new_set(journal.id(), "no-convoy-target"))
        .await?;

    let mut op = cala.begin_operation().await?;
    cala.account_sets()
        .add_member_in_op(&mut op, parent.id(), child.id())
        .await?;

    let new_account = new_account_in(target.id());
    let account_id = new_account.id;
    let account = tokio::time::timeout(MUST_COMPLETE, cala.accounts().create(new_account))
        .await
        .expect("fast-path create must not block on the exclusive structure lock")?;
    assert_eq!(account.id(), account_id);
    assert_eq!(
        direct_membership_count(&pool, target.id(), account_id).await?,
        1
    );

    op.commit().await?;
    Ok(())
}

/// An unknown target set is rejected, and the whole op (account row
/// included) rolls back.
#[tokio::test]
async fn missing_set_rejected() -> anyhow::Result<()> {
    let (cala, _jobs, _pool) = init_cala().await?;

    let missing = AccountSetId::new();
    let new_account = new_account_in(missing);
    let account_id = new_account.id;

    let res = cala.accounts().create(new_account).await;
    assert!(matches!(
        res,
        Err(AccountError::InitialAccountSetNotFound(id)) if id == missing
    ));
    assert!(matches!(
        cala.accounts().find(account_id).await,
        Err(AccountError::CouldNotFindById(_))
    ));
    Ok(())
}

/// The retained class-2 per-member EXCLUSIVE is load-bearing. A
/// concurrent *classic* attach of the same fresh account (leaked id)
/// must block on it while the fast-path op is open, and — once unblocked
/// — run its double-membership check against the committed membership
/// {S1}: re-attaching to S1 is deterministically rejected, attaching to
/// a disjoint S2 succeeds.
#[tokio::test]
async fn same_account_classic_race_blocks_on_member_lock() -> anyhow::Result<()> {
    let (cala, _jobs, pool) = init_cala().await?;
    let journal = cala.journals().create(helpers::test_journal()).await?;
    let set_one = cala
        .account_sets()
        .create(new_set(journal.id(), "race-s1"))
        .await?;
    let set_two = cala
        .account_sets()
        .create(new_set(journal.id(), "race-s2"))
        .await?;

    let new_account = new_account_in(set_one.id());
    let account_id = new_account.id;

    let mut op = cala.begin_operation().await?;
    cala.accounts().create_in_op(&mut op, new_account).await?;

    let cala2 = cala.clone();
    let set_one_id = set_one.id();
    let mut blocked = tokio::spawn(async move {
        cala2
            .account_sets()
            .add_member(set_one_id, account_id)
            .await
    });
    assert!(
        tokio::time::timeout(MUST_STILL_BE_PENDING, &mut blocked)
            .await
            .is_err(),
        "classic attach of the same fresh account must block on the class-2 lock"
    );

    op.commit().await?;

    let res = blocked.await?;
    assert!(matches!(res, Err(AccountSetError::MemberAlreadyAdded)));

    cala.account_sets()
        .add_member(set_two.id(), account_id)
        .await?;
    assert_eq!(
        direct_membership_count(&pool, set_one.id(), account_id).await?,
        1
    );
    assert_eq!(
        direct_membership_count(&pool, set_two.id(), account_id).await?,
        1
    );
    Ok(())
}

/// A fast-path create into S concurrent with a structure op adding an
/// ancestor edge above S, in BOTH commit orders. Each account ends up
/// with exactly one path to every ancestor, and the EC rollup folds its
/// postings into the (EC) ancestor exactly once.
#[tokio::test]
async fn concurrent_structure_op_yields_single_path_and_single_fold() -> anyhow::Result<()> {
    let (cala, mut jobs, pool) = init_cala().await?;
    let journal = cala.journals().create(helpers::test_journal()).await?;

    let parent_a = cala
        .account_sets()
        .create(new_ec_set(journal.id(), "order-a-parent"))
        .await?;
    let leaf_a = cala
        .account_sets()
        .create(new_set(journal.id(), "order-a-leaf"))
        .await?;
    let parent_b = cala
        .account_sets()
        .create(new_ec_set(journal.id(), "order-b-parent"))
        .await?;
    let leaf_b = cala
        .account_sets()
        .create(new_set(journal.id(), "order-b-leaf"))
        .await?;

    let mut structure_op = cala.begin_operation().await?;
    cala.account_sets()
        .add_member_in_op(&mut structure_op, parent_a.id(), leaf_a.id())
        .await?;
    let new_account = new_account_in(leaf_a.id());
    let account_a = new_account.id;
    tokio::time::timeout(MUST_COMPLETE, cala.accounts().create(new_account))
        .await
        .expect("fast-path create must not block on the open structure op")?;
    structure_op.commit().await?;

    let mut fast_op = cala.begin_operation().await?;
    let new_account = new_account_in(leaf_b.id());
    let account_b = new_account.id;
    cala.accounts()
        .create_in_op(&mut fast_op, new_account)
        .await?;
    tokio::time::timeout(
        MUST_COMPLETE,
        cala.account_sets().add_member(parent_b.id(), leaf_b.id()),
    )
    .await
    .expect("structure op must not block on the open fast-path create")?;
    fast_op.commit().await?;

    for (account_id, leaf, parent) in [
        (account_a, &leaf_a, &parent_a),
        (account_b, &leaf_b, &parent_b),
    ] {
        let paths = ancestor_path_counts(&pool, account_id).await?;
        let mut resolved: Vec<uuid::Uuid> = paths.iter().map(|(id, _)| *id).collect();
        resolved.sort();
        let mut expected = vec![uuid::Uuid::from(leaf.id()), uuid::Uuid::from(parent.id())];
        expected.sort();
        assert_eq!(
            resolved, expected,
            "account {account_id} resolved wrong ancestors"
        );
        assert!(
            paths.iter().all(|(_, n)| *n == 1),
            "account {account_id} has a double path: {paths:?}"
        );
    }

    let (sender, _) = helpers::test_accounts();
    let sender = cala.accounts().create(sender).await?;
    let tx_code = Alphanumeric.sample_string(&mut rand::rng(), 32);
    cala.tx_templates()
        .create(helpers::simple_template_with_date_default(&tx_code))
        .await?;
    let amount = dec!(42);
    for recipient in [account_a, account_b] {
        let mut params = Params::new();
        params.insert("journal_id", journal.id().to_string());
        params.insert("sender", sender.id());
        params.insert("recipient", recipient);
        params.insert("amount", amount);
        cala.post_transaction(TransactionId::new(), &tx_code, params)
            .await?;
    }
    jobs.start_poll().await?;
    let usd: Currency = "USD".parse()?;
    helpers::wait_for_settled(&cala, journal.id(), parent_a.id(), usd, amount).await?;
    helpers::wait_for_settled(&cala, journal.id(), parent_b.id(), usd, amount).await?;

    Ok(())
}

/// `AccountSets::create_*` builds set-backing accounts internally —
/// those must NOT take the fast path (no membership rows),
/// and the sets stay fully usable.
#[tokio::test]
async fn set_backing_accounts_unaffected() -> anyhow::Result<()> {
    let (cala, _jobs, pool) = init_cala().await?;
    let journal = cala.journals().create(helpers::test_journal()).await?;

    let set = cala
        .account_sets()
        .create(new_set(journal.id(), "backing-single"))
        .await?;
    let more = cala
        .account_sets()
        .create_all(vec![
            new_set(journal.id(), "backing-batch-1"),
            new_set(journal.id(), "backing-batch-2"),
        ])
        .await?;

    for set_id in std::iter::once(set.id()).chain(more.iter().map(|s| s.id())) {
        let rows = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM cala_account_set_member_accounts WHERE member_account_id = $1",
        )
        .bind(uuid::Uuid::from(set_id))
        .fetch_one(&pool)
        .await?;
        assert_eq!(
            rows, 0,
            "set-backing account {set_id} must have no membership"
        );
    }

    let new_account = new_account_in(set.id());
    let account_id = new_account.id;
    cala.accounts().create(new_account).await?;
    assert_eq!(
        direct_membership_count(&pool, set.id(), account_id).await?,
        1
    );
    Ok(())
}

/// The objid a class-2 lock on `account_id` surfaces as in `pg_locks`
/// (`hashtext(...)::bigint & 4294967295`, matching how PostgreSQL folds
/// the signed int4 hash into pg_locks' unsigned display column).
async fn class2_objid(pool: &sqlx::PgPool, account_id: AccountId) -> anyhow::Result<i64> {
    Ok(
        sqlx::query_scalar::<_, i64>("SELECT hashtext($1)::bigint & 4294967295")
            .bind(account_id.to_string())
            .fetch_one(pool)
            .await?,
    )
}

/// `(any row granted, any row waiting)` for a class-2 lock's objid. A
/// contended lock surfaces as TWO `pg_locks` rows for the same
/// (classid, objid) — one `granted = true` (the holder) and one
/// `granted = false` (the queued waiter) — so this checks for the
/// waiter's EXISTENCE rather than fetching a single (order-unspecified)
/// row.
async fn class2_lock_state(pool: &sqlx::PgPool, objid: i64) -> anyhow::Result<(bool, bool)> {
    let rows: Vec<bool> = sqlx::query_scalar(
        r#"
        SELECT granted FROM pg_locks
        WHERE locktype = 'advisory' AND classid = 2 AND objid = $1
          AND database = (SELECT oid FROM pg_database WHERE datname = current_database())
        "#,
    )
    .bind(objid)
    .fetch_all(pool)
    .await?;
    Ok((rows.iter().any(|g| *g), rows.iter().any(|g| !*g)))
}

/// The fast path's consolidated statement establishes class-2 lock
/// order ON THE PG SIDE (`ORDER BY` inside an `AS MATERIALIZED` CTE one
/// level below the volatile lock call) — never by relying on the
/// caller's array order. Proof: pre-take the HIGHER of two account ids'
/// class-2 lock from an independent session, then run a two-account
/// fast-path `create_all` covering both (built in DESCENDING id order,
/// so array order is deliberately the wrong order). If ordering were
/// caller-order (or unfenced — the CTE inlined, the lock evaluated
/// before the Sort), `create_all` would either block immediately
/// without ever holding the lower id's lock, or race
/// nondeterministically. The canonical (ascending) order predicts
/// exactly one observable state: `create_all` acquires the LOWER lock
/// (uncontended) and then blocks waiting on the HIGHER one.
#[tokio::test]
async fn fast_path_members_locked_in_canonical_order() -> anyhow::Result<()> {
    let (cala, _jobs, pool) = init_cala().await?;
    let journal = cala.journals().create(helpers::test_journal()).await?;
    let set_a = cala
        .account_sets()
        .create(new_set(journal.id(), "order-fast-a"))
        .await?;
    let set_b = cala
        .account_sets()
        .create(new_set(journal.id(), "order-fast-b"))
        .await?;

    let mut lo = new_account_in(set_a.id());
    let mut hi = new_account_in(set_b.id());
    if lo.id > hi.id {
        std::mem::swap(&mut lo, &mut hi);
    }
    let (lo_id, hi_id) = (lo.id, hi.id);
    let lo_objid = class2_objid(&pool, lo_id).await?;
    let hi_objid = class2_objid(&pool, hi_id).await?;

    let mut blocker_tx = pool.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock(2, hashtext($1::text))")
        .bind(hi_id.to_string())
        .execute(&mut *blocker_tx)
        .await?;

    let create = tokio::spawn({
        let cala = cala.clone();
        async move { cala.accounts().create_all(vec![hi, lo]).await }
    });

    tokio::time::sleep(MUST_STILL_BE_PENDING).await;
    let (lo_granted, lo_waiting) = class2_lock_state(&pool, lo_objid).await?;
    assert!(
        lo_granted && !lo_waiting,
        "canonical (ascending) order must acquire the lower id's lock \
         first (uncontended), regardless of input array order — \
         got granted={lo_granted}, waiting={lo_waiting}"
    );
    let (hi_granted, hi_waiting) = class2_lock_state(&pool, hi_objid).await?;
    assert!(
        hi_granted && hi_waiting,
        "the higher id's lock must show both the independent session's \
         hold (granted) AND the fast-path create queued behind it \
         (waiting) — got granted={hi_granted}, waiting={hi_waiting}"
    );

    blocker_tx.rollback().await?;
    let accounts = tokio::time::timeout(MUST_COMPLETE, create)
        .await
        .expect("fast-path create must complete once the higher lock is released")??;
    assert_eq!(accounts.len(), 2);

    assert_eq!(direct_membership_count(&pool, set_a.id(), lo_id).await?, 1);
    assert_eq!(direct_membership_count(&pool, set_b.id(), hi_id).await?, 1);
    Ok(())
}
