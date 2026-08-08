//! Behavioural coverage for the epoch-validated set-graph cache
//! (`account_set/graph_cache.rs`).
//!
//! The cache replaces the per-posting recursive ancestor walk with an
//! in-memory expansion over a cached edge snapshot, validated per op by
//! an epoch counter read in the same statement as the direct-membership
//! probe. These tests drive every resolution path through the public
//! posting API and assert the observable outcome (ancestor balances,
//! advisory locks) is identical to the walk's:
//!
//! - cold fallback vs warm memory-path posting parity (EC / non-EC /
//!   multi-journal fixture);
//! - same-op create+attach+post (unknown-seed supplement path — the
//!   dominant posting pattern);
//! - same-op `add_member_set` + post (epoch-mismatch op-local path),
//!   then stale-cache -> refresh -> memory-path sequence;
//! - rollback of an in-op structure change must not leak into any later
//!   resolution (op-local results are never installed — poisoning guard);
//! - cross-instance staleness: a structure change committed by another
//!   `CalaLedger` on the same database is picked up via the epoch check;
//! - ancestor advisory-lock parity between the fallback and memory
//!   paths, observed via `pg_locks` while the posting op is open.
//!
//! Lock observation and the streaming rollup (a global outbox consumer)
//! both need database isolation, so every test uses
//! `helpers::init_isolated_pool`.

mod helpers;

use std::time::Duration;

use rand::distr::{Alphanumeric, SampleString};
use rust_decimal_macros::dec;

use cala_ledger::{
    account::*, account_set::NewAccountSet, primitives::BalanceRollup, tx_template::Params, *,
};

/// How long to give the cache's background snapshot refresh (triggered
/// by a previous posting's fallback resolution) to install. The refresh
/// is a single small query against a local Postgres; one second is
/// orders of magnitude above its latency.
const CACHE_WARM: Duration = Duration::from_secs(1);

async fn init_cala(pool: sqlx::PgPool) -> anyhow::Result<(CalaLedger, job::Jobs)> {
    let mut jobs = helpers::init_jobs(pool.clone()).await?;
    let cala_config = CalaLedgerConfig::builder()
        .pool(pool)
        .exec_migrations(false)
        .build()?;
    let cala = CalaLedger::init(cala_config, &mut jobs).await?;
    Ok((cala, jobs))
}

fn new_account(name: &str, rollup: BalanceRollup) -> NewAccount {
    let code = Alphanumeric.sample_string(&mut rand::rng(), 32);
    NewAccount::builder()
        .id(uuid::Uuid::now_v7())
        .name(format!("{name} {code}"))
        .code(code)
        .balance_rollup(rollup)
        .build()
        .unwrap()
}

fn new_set(journal_id: JournalId, name: &str, rollup: BalanceRollup) -> NewAccountSet {
    let code = Alphanumeric.sample_string(&mut rand::rng(), 32);
    NewAccountSet::builder()
        .id(uuid::Uuid::now_v7())
        .name(format!("{name} {code}"))
        .journal_id(journal_id)
        .balance_rollup(rollup)
        .build()
        .unwrap()
}

async fn create_template(cala: &CalaLedger) -> anyhow::Result<String> {
    let tx_code = Alphanumeric.sample_string(&mut rand::rng(), 32);
    cala.tx_templates()
        .create(helpers::simple_template_with_date_default(&tx_code))
        .await?;
    Ok(tx_code)
}

fn posting_params(
    journal_id: JournalId,
    sender: AccountId,
    recipient: AccountId,
    amount: rust_decimal::Decimal,
) -> Params {
    let mut params = Params::new();
    params.insert("journal_id", journal_id.to_string());
    params.insert("sender", sender);
    params.insert("recipient", recipient);
    params.insert("amount", amount);
    params
}

/// A depth-3 chain plus an other-journal parent:
/// `member -> s1 (sync) -> s2 (EC) -> s3 (sync)`, and `member` also
/// directly in `other_journal_set` (different journal — walked through
/// but excluded from resolution, exactly like the walk SQL).
struct Chain {
    member: AccountId,
    counter: AccountId,
    s1: AccountSetId,
    s2_ec: AccountSetId,
    s3: AccountSetId,
    other_journal_set: AccountSetId,
}

async fn build_chain(
    cala: &CalaLedger,
    journal_id: JournalId,
    other_journal_id: JournalId,
) -> anyhow::Result<Chain> {
    let member = cala
        .accounts()
        .create(new_account("member", BalanceRollup::Synchronous))
        .await?;
    let counter = cala
        .accounts()
        .create(new_account("counter", BalanceRollup::Synchronous))
        .await?;
    let s1 = cala
        .account_sets()
        .create(new_set(journal_id, "s1", BalanceRollup::Synchronous))
        .await?;
    let s2_ec = cala
        .account_sets()
        .create(new_set(
            journal_id,
            "s2 ec",
            BalanceRollup::EventuallyConsistent,
        ))
        .await?;
    let s3 = cala
        .account_sets()
        .create(new_set(journal_id, "s3", BalanceRollup::Synchronous))
        .await?;
    let other_journal_set = cala
        .account_sets()
        .create(new_set(
            other_journal_id,
            "other journal",
            BalanceRollup::Synchronous,
        ))
        .await?;

    cala.account_sets().add_member(s1.id(), member.id()).await?;
    cala.account_sets()
        .add_member(other_journal_set.id(), member.id())
        .await?;
    cala.account_sets().add_member(s2_ec.id(), s1.id()).await?;
    cala.account_sets().add_member(s3.id(), s2_ec.id()).await?;

    Ok(Chain {
        member: member.id(),
        counter: counter.id(),
        s1: s1.id(),
        s2_ec: s2_ec.id(),
        s3: s3.id(),
        other_journal_set: other_journal_set.id(),
    })
}

/// Cold (fallback-walk) and warm (in-memory) postings must produce
/// identical ancestor fan-out: synchronous ancestors inline, the EC
/// ancestor via the streaming rollup, the other-journal parent never.
#[tokio::test]
async fn warm_resolution_matches_walk_fallback() -> anyhow::Result<()> {
    let pool = helpers::init_isolated_pool().await?;
    let (cala, mut jobs) = init_cala(pool).await?;

    let journal = cala.journals().create(helpers::test_journal()).await?;
    let other_journal = cala.journals().create(helpers::test_journal()).await?;
    let tx_code = create_template(&cala).await?;
    let usd = "USD".parse::<Currency>()?;

    let chain = build_chain(&cala, journal.id(), other_journal.id()).await?;

    // Posting 1: cold cache -> op-local walk fallback (and triggers the
    // installing background refresh).
    cala.post_transaction(
        TransactionId::new(),
        &tx_code,
        posting_params(journal.id(), chain.counter, chain.member, dec!(7)),
    )
    .await?;
    for set_id in [chain.s1, chain.s3] {
        assert_eq!(
            cala.balances()
                .find(journal.id(), set_id, usd)
                .await?
                .settled(),
            dec!(7),
            "cold-path posting must fan into every synchronous ancestor"
        );
    }

    // Posting 2: warm cache -> in-memory expansion.
    tokio::time::sleep(CACHE_WARM).await;
    cala.post_transaction(
        TransactionId::new(),
        &tx_code,
        posting_params(journal.id(), chain.counter, chain.member, dec!(7)),
    )
    .await?;
    for set_id in [chain.s1, chain.s3] {
        assert_eq!(
            cala.balances()
                .find(journal.id(), set_id, usd)
                .await?
                .settled(),
            dec!(14),
            "warm-path posting must fan into the same synchronous ancestors"
        );
    }

    // The other-journal parent is excluded by both paths.
    assert!(
        cala.balances()
            .find(journal.id(), chain.other_journal_set, usd)
            .await
            .is_err(),
        "a set in another journal must never receive balance"
    );
    assert!(
        cala.balances()
            .find(other_journal.id(), chain.other_journal_set, usd)
            .await
            .is_err(),
        "no entries were posted in the other journal"
    );

    // The EC ancestor gets both postings via the streaming rollup —
    // which resolves through the same cache.
    jobs.start_poll().await?;
    helpers::wait_for_settled(&cala, journal.id(), chain.s2_ec, usd, dec!(14)).await?;

    Ok(())
}

/// A dominant posting pattern: create a set, attach a fresh account, and
/// post to it — all in one op. No epoch bump happens (account-member
/// adds don't touch the edge graph), so the warm cache takes the
/// unknown-seed supplement path and must still resolve the fresh set.
#[tokio::test]
async fn same_op_create_attach_post_resolves_fresh_set() -> anyhow::Result<()> {
    let pool = helpers::init_isolated_pool().await?;
    let (cala, _jobs) = init_cala(pool).await?;

    let journal = cala.journals().create(helpers::test_journal()).await?;
    let other_journal = cala.journals().create(helpers::test_journal()).await?;
    let tx_code = create_template(&cala).await?;
    let usd = "USD".parse::<Currency>()?;

    // Warm the cache with an unrelated posting.
    let chain = build_chain(&cala, journal.id(), other_journal.id()).await?;
    cala.post_transaction(
        TransactionId::new(),
        &tx_code,
        posting_params(journal.id(), chain.counter, chain.member, dec!(1)),
    )
    .await?;
    tokio::time::sleep(CACHE_WARM).await;

    // One op: create account + set, attach, post.
    let mut op = cala.begin_operation().await?;
    let fresh_account = cala
        .accounts()
        .create_in_op(&mut op, new_account("fresh", BalanceRollup::Synchronous))
        .await?;
    let fresh_set = cala
        .account_sets()
        .create_in_op(
            &mut op,
            new_set(journal.id(), "fresh set", BalanceRollup::Synchronous),
        )
        .await?;
    cala.account_sets()
        .add_member_in_op(&mut op, fresh_set.id(), fresh_account.id())
        .await?;
    cala.post_transaction_in_op(
        &mut op,
        TransactionId::new(),
        &tx_code,
        posting_params(journal.id(), chain.counter, fresh_account.id(), dec!(3)),
    )
    .await?;
    op.commit().await?;

    assert_eq!(
        cala.balances()
            .find(journal.id(), fresh_set.id(), usd)
            .await?
            .settled(),
        dec!(3),
        "a set created+attached+posted in one op must receive the posting inline"
    );

    Ok(())
}

/// A same-op `add_member_set` bumps the epoch inside the op, so the
/// posting's probe sees a mismatch and resolves op-locally (observing
/// the uncommitted edge). After commit the shared cache is stale ->
/// epoch fallback -> refresh -> memory path; each posting must fan out
/// identically.
#[tokio::test]
async fn same_op_add_member_set_then_stale_then_warm() -> anyhow::Result<()> {
    let pool = helpers::init_isolated_pool().await?;
    let (cala, _jobs) = init_cala(pool).await?;

    let journal = cala.journals().create(helpers::test_journal()).await?;
    let other_journal = cala.journals().create(helpers::test_journal()).await?;
    let tx_code = create_template(&cala).await?;
    let usd = "USD".parse::<Currency>()?;

    let chain = build_chain(&cala, journal.id(), other_journal.id()).await?;

    // Fresh subtree with no history (the freeze guard only fences
    // members that already have activity): account b -> set t.
    let b = cala
        .accounts()
        .create(new_account("b", BalanceRollup::Synchronous))
        .await?;
    let t = cala
        .account_sets()
        .create(new_set(journal.id(), "t", BalanceRollup::Synchronous))
        .await?;
    cala.account_sets().add_member(t.id(), b.id()).await?;

    // Warm the cache (t and its membership become part of the snapshot).
    cala.post_transaction(
        TransactionId::new(),
        &tx_code,
        posting_params(journal.id(), chain.counter, chain.member, dec!(1)),
    )
    .await?;
    tokio::time::sleep(CACHE_WARM).await;

    // One op: graft t under s1 (epoch bump, uncommitted) + post to b.
    // The op-local resolution must already see b's new ancestors.
    let mut op = cala.begin_operation().await?;
    cala.account_sets()
        .add_member_in_op(&mut op, chain.s1, t.id())
        .await?;
    cala.post_transaction_in_op(
        &mut op,
        TransactionId::new(),
        &tx_code,
        posting_params(journal.id(), chain.counter, b.id(), dec!(5)),
    )
    .await?;
    op.commit().await?;

    assert_eq!(
        cala.balances()
            .find(journal.id(), t.id(), usd)
            .await?
            .settled(),
        dec!(5)
    );
    assert_eq!(
        cala.balances()
            .find(journal.id(), chain.s1, usd)
            .await?
            .settled(),
        dec!(6),
        "the same-op posting must fan through the edge added in the same op"
    );

    // Posting 2: shared cache is stale (committed epoch moved) ->
    // epoch-mismatch fallback, still correct.
    cala.post_transaction(
        TransactionId::new(),
        &tx_code,
        posting_params(journal.id(), chain.counter, b.id(), dec!(5)),
    )
    .await?;
    assert_eq!(
        cala.balances()
            .find(journal.id(), chain.s1, usd)
            .await?
            .settled(),
        dec!(11)
    );

    // Posting 3: refresh installed -> memory path, same fan-out.
    tokio::time::sleep(CACHE_WARM).await;
    cala.post_transaction(
        TransactionId::new(),
        &tx_code,
        posting_params(journal.id(), chain.counter, b.id(), dec!(5)),
    )
    .await?;
    assert_eq!(
        cala.balances()
            .find(journal.id(), chain.s1, usd)
            .await?
            .settled(),
        dec!(16)
    );
    assert_eq!(
        cala.balances()
            .find(journal.id(), chain.s3, usd)
            .await?
            .settled(),
        dec!(16),
        "the whole chain above the grafted subtree must see every posting"
    );

    Ok(())
}

/// A rolled-back op that mutated the edge graph (and resolved through
/// the mutated graph op-locally) must leave no trace: the shared cache
/// only ever installs committed data, so the next posting must resolve
/// against the pre-rollback graph.
#[tokio::test]
async fn rolled_back_structure_change_does_not_poison_cache() -> anyhow::Result<()> {
    let pool = helpers::init_isolated_pool().await?;
    let (cala, _jobs) = init_cala(pool).await?;

    let journal = cala.journals().create(helpers::test_journal()).await?;
    let other_journal = cala.journals().create(helpers::test_journal()).await?;
    let tx_code = create_template(&cala).await?;
    let usd = "USD".parse::<Currency>()?;

    let chain = build_chain(&cala, journal.id(), other_journal.id()).await?;

    let b = cala
        .accounts()
        .create(new_account("b", BalanceRollup::Synchronous))
        .await?;
    let t = cala
        .account_sets()
        .create(new_set(journal.id(), "t", BalanceRollup::Synchronous))
        .await?;
    cala.account_sets().add_member(t.id(), b.id()).await?;

    // Warm the cache.
    cala.post_transaction(
        TransactionId::new(),
        &tx_code,
        posting_params(journal.id(), chain.counter, chain.member, dec!(1)),
    )
    .await?;
    tokio::time::sleep(CACHE_WARM).await;
    let s1_before = cala
        .balances()
        .find(journal.id(), chain.s1, usd)
        .await?
        .settled();

    // Graft t under s1 and post to b — then ROLL BACK the whole op.
    let mut op = cala.begin_operation().await?;
    cala.account_sets()
        .add_member_in_op(&mut op, chain.s1, t.id())
        .await?;
    cala.post_transaction_in_op(
        &mut op,
        TransactionId::new(),
        &tx_code,
        posting_params(journal.id(), chain.counter, b.id(), dec!(9)),
    )
    .await?;
    drop(op); // rollback

    // Give any background refresh the rolled-back op may have triggered
    // time to run — it must only ever see committed state.
    tokio::time::sleep(CACHE_WARM).await;

    // A committed posting to b must fan into t only — never into s1.
    cala.post_transaction(
        TransactionId::new(),
        &tx_code,
        posting_params(journal.id(), chain.counter, b.id(), dec!(9)),
    )
    .await?;
    assert_eq!(
        cala.balances()
            .find(journal.id(), t.id(), usd)
            .await?
            .settled(),
        dec!(9)
    );
    assert_eq!(
        cala.balances()
            .find(journal.id(), chain.s1, usd)
            .await?
            .settled(),
        s1_before,
        "the rolled-back edge must not influence any later resolution"
    );

    Ok(())
}

/// Two `CalaLedger` instances (separate in-process caches) on one
/// database: a structure change committed through instance 2 must be
/// observed by instance 1's next posting via the epoch check — there is
/// no cross-instance invalidation channel, and none is needed.
#[tokio::test]
async fn cross_instance_structure_change_is_observed() -> anyhow::Result<()> {
    let pool = helpers::init_isolated_pool().await?;
    let (cala1, _jobs1) = init_cala(pool.clone()).await?;
    let (cala2, _jobs2) = init_cala(pool).await?;

    let journal = cala1.journals().create(helpers::test_journal()).await?;
    let other_journal = cala1.journals().create(helpers::test_journal()).await?;
    let tx_code = create_template(&cala1).await?;
    let usd = "USD".parse::<Currency>()?;

    let chain = build_chain(&cala1, journal.id(), other_journal.id()).await?;

    let b = cala1
        .accounts()
        .create(new_account("b", BalanceRollup::Synchronous))
        .await?;
    let t = cala1
        .account_sets()
        .create(new_set(journal.id(), "t", BalanceRollup::Synchronous))
        .await?;
    cala1.account_sets().add_member(t.id(), b.id()).await?;

    // Warm instance 1's cache with a posting that leaves t's subtree
    // untouched (activity under t would freeze it against the graft).
    cala1
        .post_transaction(
            TransactionId::new(),
            &tx_code,
            posting_params(journal.id(), chain.counter, chain.member, dec!(2)),
        )
        .await?;
    tokio::time::sleep(CACHE_WARM).await;

    // Instance 2 grafts t under s1 (epoch bump commits).
    cala2.account_sets().add_member(chain.s1, t.id()).await?;

    // Instance 1's cache is now stale; its next posting must observe
    // the new edge through the epoch check and fan into s1.
    cala1
        .post_transaction(
            TransactionId::new(),
            &tx_code,
            posting_params(journal.id(), chain.counter, b.id(), dec!(2)),
        )
        .await?;
    assert_eq!(
        cala1
            .balances()
            .find(journal.id(), chain.s1, usd)
            .await?
            .settled(),
        dec!(4),
        "a structure change committed by another instance must be visible immediately"
    );
    assert_eq!(
        cala1
            .balances()
            .find(journal.id(), t.id(), usd)
            .await?
            .settled(),
        dec!(2)
    );

    Ok(())
}

/// `true` iff a session currently holds the 1-arg per-balance advisory
/// exclusive for `(journal, target, currency)` — the same key shape the
/// poster takes on non-EC ancestor sets. `pg_locks` splits the 64-bit
/// key into `(classid, objid)` with `objsubid = 1` for the 1-arg form;
/// `hashtext(..)::bigint` sign-extends, so compare the halves.
async fn per_balance_exclusive_held(
    pool: &sqlx::PgPool,
    journal_id: JournalId,
    target: impl Into<AccountId>,
    currency: Currency,
) -> anyhow::Result<bool> {
    let key = format!("{}{}{}", journal_id, target.into(), currency.code());
    let held = sqlx::query_scalar::<_, bool>(
        r#"
        SELECT EXISTS (
            SELECT 1 FROM pg_locks
            WHERE locktype = 'advisory'
            AND objsubid = 1
            AND mode = 'ExclusiveLock'
            AND database = (SELECT oid FROM pg_database WHERE datname = current_database())
            AND classid::bigint = ((hashtext($1)::bigint >> 32) & 4294967295)
            AND objid::bigint = (hashtext($1)::bigint & 4294967295)
        )
        "#,
    )
    .bind(key)
    .fetch_one(pool)
    .await?;
    Ok(held)
}

/// Lock parity between the fallback and memory paths: with a posting op
/// held open, the non-EC ancestors' per-balance exclusives must be held
/// and the EC ancestor's must not — identically on the cold (walk) and
/// warm (in-memory) resolutions.
#[tokio::test]
async fn ancestor_lock_parity_between_fallback_and_memory_paths() -> anyhow::Result<()> {
    let pool = helpers::init_isolated_pool().await?;
    let (cala, _jobs) = init_cala(pool.clone()).await?;

    let journal = cala.journals().create(helpers::test_journal()).await?;
    let other_journal = cala.journals().create(helpers::test_journal()).await?;
    let tx_code = create_template(&cala).await?;
    let usd = "USD".parse::<Currency>()?;

    let chain = build_chain(&cala, journal.id(), other_journal.id()).await?;

    for (pass, warm) in [("cold/fallback", false), ("warm/memory", true)] {
        if warm {
            tokio::time::sleep(CACHE_WARM).await;
        }
        let mut op = cala.begin_operation().await?;
        cala.post_transaction_in_op(
            &mut op,
            TransactionId::new(),
            &tx_code,
            posting_params(journal.id(), chain.counter, chain.member, dec!(1)),
        )
        .await?;

        for set_id in [chain.s1, chain.s3] {
            assert!(
                per_balance_exclusive_held(&pool, journal.id(), set_id, usd).await?,
                "{pass}: the non-EC ancestor's per-balance exclusive must be held"
            );
        }
        assert!(
            !per_balance_exclusive_held(&pool, journal.id(), chain.s2_ec, usd).await?,
            "{pass}: the EC ancestor must not be per-balance locked (rollup owns it)"
        );
        assert!(
            !per_balance_exclusive_held(&pool, journal.id(), chain.other_journal_set, usd).await?,
            "{pass}: an other-journal parent must never be locked"
        );

        op.commit().await?;
    }

    Ok(())
}
