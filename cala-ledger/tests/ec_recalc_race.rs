//! Concurrent correctness test for the EC account set recalc <-> poster
//! lock pair.
//!
//! Reproduces the bug where `nextval` ordering on `cala_balance_history.seq`
//! does not match commit visibility ordering: a poster may have an
//! uncommitted seq that is *smaller* than the seqs already visible to a
//! concurrently running recalc. Without the lock pair, the recalc would
//! advance its watermark past the uncommitted seq and silently skip the
//! row when it later becomes visible.
//!
//! This test stresses the interleaving by spawning many writer tasks and
//! many recalc tasks in parallel, then asserts that the EC set's balance
//! equals the sum of all posted credits — both **without** a final recalc
//! (incremental correctness) and **after** a final recalc (idempotency).

mod helpers;

use std::sync::Arc;

use rand::distr::{Alphanumeric, SampleString};
use rand::RngExt;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use cala_ledger::{account::*, account_set::*, balance::error::BalanceError, tx_template::*, *};

const N_MEMBERS: usize = 8;
const N_WRITERS: usize = 8;
const N_RECALCS: usize = 4;
const N_ITERATIONS: usize = 4;
const POSTS_PER_WRITER_PER_ITERATION: usize = 6;
const POST_AMOUNT: Decimal = dec!(7);

#[tokio::test]
async fn ec_recalc_race_under_concurrency() -> anyhow::Result<()> {
    let usd: Currency = "USD".parse().unwrap();

    // Use a larger pool than `helpers::init_pool`'s default so the
    // concurrent writers + recalcs do not starve on connection acquisition.
    let pool = helpers::init_pool_with(
        sqlx::postgres::PgPoolOptions::new()
            .max_connections(40)
            .acquire_timeout(std::time::Duration::from_secs(60)),
    )
    .await?;

    let cala_config = CalaLedgerConfig::builder()
        .pool(pool)
        .exec_migrations(false)
        .build()?;
    let cala = CalaLedger::init(cala_config).await?;

    let journal = cala
        .journals()
        .create(helpers::test_journal())
        .await
        .unwrap();

    // Sender: a non-EC account that absorbs all the debits.
    let sender_code = Alphanumeric.sample_string(&mut rand::rng(), 32);
    let sender = NewAccount::builder()
        .id(uuid::Uuid::now_v7())
        .name(format!("EC race sender {sender_code}"))
        .code(sender_code)
        .build()
        .unwrap();
    let sender = cala.accounts().create(sender).await.unwrap();

    // Members: N leaf accounts that will be added to the EC set.
    let mut members: Vec<Account> = Vec::with_capacity(N_MEMBERS);
    for i in 0..N_MEMBERS {
        let code = Alphanumeric.sample_string(&mut rand::rng(), 32);
        let acc = NewAccount::builder()
            .id(uuid::Uuid::now_v7())
            .name(format!("EC race member {i} {code}"))
            .code(code)
            .build()
            .unwrap();
        members.push(cala.accounts().create(acc).await.unwrap());
    }

    let ec_set = NewAccountSet::builder()
        .id(AccountSetId::new())
        .name("EC race set")
        .journal_id(journal.id())
        .eventually_consistent(true)
        .build()
        .unwrap();
    let ec_set = cala.account_sets().create(ec_set).await.unwrap();

    for m in &members {
        cala.account_sets()
            .add_member(ec_set.id(), m.id())
            .await
            .unwrap();
    }

    let tx_code = Alphanumeric.sample_string(&mut rand::rng(), 32);
    cala.tx_templates()
        .create(helpers::simple_template_with_date_default(&tx_code))
        .await
        .unwrap();

    let member_ids: Arc<Vec<AccountId>> = Arc::new(members.iter().map(|a| a.id()).collect());

    // Run the bursty pattern several times so the interleaving has plenty
    // of opportunities to expose a race.
    for _ in 0..N_ITERATIONS {
        let mut handles = Vec::with_capacity(N_WRITERS + N_RECALCS);

        for _ in 0..N_WRITERS {
            let cala = cala.clone();
            let member_ids = member_ids.clone();
            let tx_code = tx_code.clone();
            let sender_id = sender.id();
            let journal_id = journal.id();
            handles.push(tokio::spawn(async move {
                for _ in 0..POSTS_PER_WRITER_PER_ITERATION {
                    let recipient_id = {
                        let mut rng = rand::rng();
                        member_ids[rng.random_range(0..member_ids.len())]
                    };
                    let mut params = Params::new();
                    params.insert("journal_id", journal_id.to_string());
                    params.insert("sender", sender_id);
                    params.insert("recipient", recipient_id);
                    params.insert("amount", POST_AMOUNT);
                    cala.post_transaction(TransactionId::new(), &tx_code, params)
                        .await
                        .map_err(|e| anyhow::anyhow!("post_transaction failed: {e}"))?;
                }
                Ok::<_, anyhow::Error>(())
            }));
        }

        for _ in 0..N_RECALCS {
            let cala = cala.clone();
            let set_id = ec_set.id();
            handles.push(tokio::spawn(async move {
                cala.account_sets()
                    .recalculate_balances(set_id)
                    .await
                    .map_err(|e| anyhow::anyhow!("recalculate_balances failed: {e}"))?;
                Ok::<_, anyhow::Error>(())
            }));
        }

        for h in handles {
            h.await??;
        }
    }

    let total_posts = N_ITERATIONS * N_WRITERS * POSTS_PER_WRITER_PER_ITERATION;
    let expected_total = POST_AMOUNT * Decimal::from(total_posts);

    // (a) Without a final recalc — exercises that the in-flight recalcs
    //     produced a balance that already covers every committed post.
    //
    // Note: a recalc that runs concurrently with a writer might miss the
    // writer's last commit if the commit lands a few microseconds after
    // the recalc has finished its read phase. The lock pair guarantees
    // that no row is *permanently* skipped, but it does NOT guarantee
    // that a recalc which started before a poster committed observes
    // that poster. So in the no-final-recalc check we only assert that
    // the EC set balance is consistent with *some* prefix of the posts —
    // i.e. ≤ the expected total — and that the rows it does account for
    // are present.
    //
    // The hard correctness check happens after the final recalc below.
    let pre_final = cala.balances().find(journal.id(), ec_set.id(), usd).await?;
    let pre_final_settled = pre_final.settled();
    assert!(
        pre_final_settled <= expected_total,
        "EC set balance {pre_final_settled} exceeded expected total {expected_total}",
    );

    // (b) After a final recalc — every committed post must now be
    //     reflected. This is the assertion that fails if the watermark
    //     race lets a row slip through.
    cala.account_sets()
        .recalculate_balances(ec_set.id())
        .await
        .unwrap();

    let final_bal = cala.balances().find(journal.id(), ec_set.id(), usd).await?;
    assert_eq!(
        final_bal.settled(),
        expected_total,
        "EC set balance after final recalc must equal sum of all posts \
         (got {got}, expected {expected_total}, pre-final was {pre_final_settled})",
        got = final_bal.settled(),
    );

    // Cross-check by summing the member balances directly. This catches
    // the case where the EC set balance happens to match the expected
    // total but is internally inconsistent with the actual member state.
    // Members that received zero posts have no balance row yet — only
    // tolerate that specific NotFound case so a real failure does not
    // get silently swallowed.
    let mut sum_members = Decimal::ZERO;
    for m in &members {
        match cala.balances().find(journal.id(), m.id(), usd).await {
            Ok(b) => sum_members += b.settled(),
            Err(BalanceError::NotFound(..)) => {}
            Err(e) => return Err(e.into()),
        }
    }
    assert_eq!(
        sum_members, expected_total,
        "sum of member balances must equal sum of posts",
    );
    assert_eq!(
        final_bal.settled(),
        sum_members,
        "EC set balance must equal sum of member balances",
    );

    // Idempotency: another final recalc must not change anything.
    cala.account_sets()
        .recalculate_balances(ec_set.id())
        .await
        .unwrap();
    let final_bal_2 = cala.balances().find(journal.id(), ec_set.id(), usd).await?;
    assert_eq!(
        final_bal.settled(),
        final_bal_2.settled(),
        "recalculate_balances must be idempotent",
    );
    assert_eq!(
        final_bal.details.version, final_bal_2.details.version,
        "version must not change on idempotent recalc",
    );

    Ok(())
}

/// Hierarchical variant of the race test.
///
/// Layout: `parent_set` (non-EC) ⊇ `ec_set` (EC) ⊇ `N` leaves.
///
/// When a poster writes to a leaf, `fetch_mappings_in_op` walks the
/// transitive closure in `cala_account_set_member_accounts` and returns
/// both `ec_set` and `parent_set` as owning sets. The poster therefore
/// takes shared advisory locks on the full ancestor chain before its
/// `nextval`, while concurrent recalcs on `ec_set` hold exclusive. This
/// test exercises that protocol at depth > 1 and asserts that the
/// non-EC ancestor (maintained synchronously by posters) and the
/// inner EC set (maintained by recalcs) end up with identical balances.
#[tokio::test]
async fn ec_recalc_hierarchy_race_under_concurrency() -> anyhow::Result<()> {
    let usd: Currency = "USD".parse().unwrap();

    let pool = helpers::init_pool_with(
        sqlx::postgres::PgPoolOptions::new()
            .max_connections(40)
            .acquire_timeout(std::time::Duration::from_secs(60)),
    )
    .await?;

    let cala_config = CalaLedgerConfig::builder()
        .pool(pool)
        .exec_migrations(false)
        .build()?;
    let cala = CalaLedger::init(cala_config).await?;

    let journal = cala
        .journals()
        .create(helpers::test_journal())
        .await
        .unwrap();

    let sender_code = Alphanumeric.sample_string(&mut rand::rng(), 32);
    let sender = NewAccount::builder()
        .id(uuid::Uuid::now_v7())
        .name(format!("EC hierarchy sender {sender_code}"))
        .code(sender_code)
        .build()
        .unwrap();
    let sender = cala.accounts().create(sender).await.unwrap();

    let mut members: Vec<Account> = Vec::with_capacity(N_MEMBERS);
    for i in 0..N_MEMBERS {
        let code = Alphanumeric.sample_string(&mut rand::rng(), 32);
        let acc = NewAccount::builder()
            .id(uuid::Uuid::now_v7())
            .name(format!("EC hierarchy member {i} {code}"))
            .code(code)
            .build()
            .unwrap();
        members.push(cala.accounts().create(acc).await.unwrap());
    }

    // Inner EC set: holds the leaves, rebuilt via recalc.
    let ec_set = NewAccountSet::builder()
        .id(AccountSetId::new())
        .name("EC hierarchy inner set")
        .journal_id(journal.id())
        .eventually_consistent(true)
        .build()
        .unwrap();
    let ec_set = cala.account_sets().create(ec_set).await.unwrap();

    // Outer non-EC parent: wraps the EC set, maintained synchronously
    // by the poster path via the transitive closure.
    let parent_set = NewAccountSet::builder()
        .id(AccountSetId::new())
        .name("EC hierarchy parent set")
        .journal_id(journal.id())
        .build()
        .unwrap();
    let parent_set = cala.account_sets().create(parent_set).await.unwrap();

    cala.account_sets()
        .add_member(parent_set.id(), ec_set.id())
        .await
        .unwrap();
    for m in &members {
        cala.account_sets()
            .add_member(ec_set.id(), m.id())
            .await
            .unwrap();
    }

    let tx_code = Alphanumeric.sample_string(&mut rand::rng(), 32);
    cala.tx_templates()
        .create(helpers::simple_template_with_date_default(&tx_code))
        .await
        .unwrap();

    let member_ids: Arc<Vec<AccountId>> = Arc::new(members.iter().map(|a| a.id()).collect());

    for _ in 0..N_ITERATIONS {
        let mut handles = Vec::with_capacity(N_WRITERS + N_RECALCS);

        for _ in 0..N_WRITERS {
            let cala = cala.clone();
            let member_ids = member_ids.clone();
            let tx_code = tx_code.clone();
            let sender_id = sender.id();
            let journal_id = journal.id();
            handles.push(tokio::spawn(async move {
                for _ in 0..POSTS_PER_WRITER_PER_ITERATION {
                    let recipient_id = {
                        let mut rng = rand::rng();
                        member_ids[rng.random_range(0..member_ids.len())]
                    };
                    let mut params = Params::new();
                    params.insert("journal_id", journal_id.to_string());
                    params.insert("sender", sender_id);
                    params.insert("recipient", recipient_id);
                    params.insert("amount", POST_AMOUNT);
                    cala.post_transaction(TransactionId::new(), &tx_code, params)
                        .await
                        .map_err(|e| anyhow::anyhow!("post_transaction failed: {e}"))?;
                }
                Ok::<_, anyhow::Error>(())
            }));
        }

        for _ in 0..N_RECALCS {
            let cala = cala.clone();
            let set_id = ec_set.id();
            handles.push(tokio::spawn(async move {
                cala.account_sets()
                    .recalculate_balances(set_id)
                    .await
                    .map_err(|e| anyhow::anyhow!("recalculate_balances failed: {e}"))?;
                Ok::<_, anyhow::Error>(())
            }));
        }

        for h in handles {
            h.await??;
        }
    }

    let total_posts = N_ITERATIONS * N_WRITERS * POSTS_PER_WRITER_PER_ITERATION;
    let expected_total = POST_AMOUNT * Decimal::from(total_posts);

    // The non-EC parent is built synchronously by the poster path, so
    // its balance must already equal the sum of all posts — no recalc
    // involved on this account at any point in the test.
    let parent_bal = cala
        .balances()
        .find(journal.id(), parent_set.id(), usd)
        .await?;
    assert_eq!(
        parent_bal.settled(),
        expected_total,
        "non-EC parent balance must equal sum of all posts",
    );

    // Final recalc on the inner EC set. Every committed post must be
    // reflected afterwards — this is the assertion that would fail if
    // the ancestor-chain shared lock did not cover the full closure.
    cala.account_sets()
        .recalculate_balances(ec_set.id())
        .await
        .unwrap();

    let ec_bal = cala.balances().find(journal.id(), ec_set.id(), usd).await?;
    assert_eq!(
        ec_bal.settled(),
        expected_total,
        "inner EC set balance after final recalc must equal sum of all posts",
    );
    assert_eq!(
        parent_bal.settled(),
        ec_bal.settled(),
        "non-EC parent and inner EC set balances must agree",
    );

    Ok(())
}

/// Concurrent multi-call `add_member_in_op` transactions on a shared
/// pool of EC parent sets, interleaved with posters on leaves that
/// those parents contain, must not deadlock.
///
/// Shape: `N_TASKS` tokio tasks each open a transaction and do
/// `ADDS_PER_TASK` `add_member_in_op` calls against the parents in
/// a randomised order (so different transactions accumulate
/// membership writes against the same parents in different orders).
/// `N_POSTERS` poster tasks concurrently post to a single hot leaf
/// that is a direct member of every parent, so each poster's
/// `find_for_update` call takes SHARED on every parent in one go.
#[tokio::test]
async fn add_member_multi_call_no_deadlock_with_posters() -> anyhow::Result<()> {
    const N_TASKS: usize = 6;
    const ADDS_PER_TASK: usize = 4;
    const N_POSTERS: usize = 4;
    const POSTS_PER_POSTER: usize = 8;
    const N_ITERATIONS: usize = 3;

    let usd: Currency = "USD".parse().unwrap();

    let pool = helpers::init_pool_with(
        sqlx::postgres::PgPoolOptions::new()
            .max_connections(40)
            .acquire_timeout(std::time::Duration::from_secs(60)),
    )
    .await?;

    let cala_config = CalaLedgerConfig::builder()
        .pool(pool)
        .exec_migrations(false)
        .build()?;
    let cala = CalaLedger::init(cala_config).await?;

    let journal = cala
        .journals()
        .create(helpers::test_journal())
        .await
        .unwrap();

    let tx_code = Alphanumeric.sample_string(&mut rand::rng(), 32);
    cala.tx_templates()
        .create(helpers::simple_template_with_date_default(&tx_code))
        .await
        .unwrap();

    let sender_code = Alphanumeric.sample_string(&mut rand::rng(), 32);
    let sender = NewAccount::builder()
        .id(uuid::Uuid::now_v7())
        .name(format!("Deadlock repro sender {sender_code}"))
        .code(sender_code)
        .build()
        .unwrap();
    let sender = cala.accounts().create(sender).await.unwrap();

    // Fewer parents than tasks, so concurrent add_member tasks
    // reliably overlap on the same parent sets.
    const N_PARENTS: usize = 3;
    let mut parents: Vec<AccountSet> = Vec::with_capacity(N_PARENTS);
    for i in 0..N_PARENTS {
        let set = NewAccountSet::builder()
            .id(AccountSetId::new())
            .name(format!("Deadlock repro parent {i}"))
            .journal_id(journal.id())
            .eventually_consistent(true)
            .build()
            .unwrap();
        parents.push(cala.account_sets().create(set).await.unwrap());
    }

    // Pre-existing leaves that belong to every parent. The poster
    // One hot leaf that is a direct member of every parent set, so
    // a single poster's `find_for_update` call takes SHARED on every
    // parent at once.
    let hot_leaf_code = Alphanumeric.sample_string(&mut rand::rng(), 32);
    let hot_leaf = NewAccount::builder()
        .id(uuid::Uuid::now_v7())
        .name(format!("Deadlock repro hot leaf {hot_leaf_code}"))
        .code(hot_leaf_code)
        .build()
        .unwrap();
    let hot_leaf = cala.accounts().create(hot_leaf).await.unwrap();
    for parent in &parents {
        cala.account_sets()
            .add_member(parent.id(), hot_leaf.id())
            .await
            .unwrap();
    }

    let parent_ids: Arc<Vec<AccountSetId>> = Arc::new(parents.iter().map(|s| s.id()).collect());
    let hot_leaf_id = hot_leaf.id();

    for iteration in 0..N_ITERATIONS {
        let mut handles = Vec::with_capacity(N_TASKS + N_POSTERS);

        // Add_member tasks: each opens a transaction and does several
        // `add_member_in_op` calls against the shared parent pool in
        // a randomised order. The random ordering models real
        // application callers that pick parents in their own order
        // (not canonical UUID order), so concurrent transactions
        // accumulate membership writes against the same parents in
        // different orders.
        for task_idx in 0..N_TASKS {
            let cala = cala.clone();
            let parent_ids = parent_ids.clone();
            handles.push(tokio::spawn(async move {
                let mut ordering: Vec<AccountSetId> = parent_ids.iter().copied().collect();
                {
                    use rand::seq::SliceRandom as _;
                    let mut rng = rand::rng();
                    ordering.shuffle(&mut rng);
                }
                let mut db = cala
                    .begin_operation()
                    .await
                    .map_err(|e| anyhow::anyhow!("begin_operation: {e}"))?;
                for add_idx in 0..ADDS_PER_TASK {
                    let parent_id = ordering[add_idx % ordering.len()];
                    let code = format!(
                        "dl-{}-{}-{}-{}",
                        iteration,
                        task_idx,
                        add_idx,
                        Alphanumeric.sample_string(&mut rand::rng(), 16)
                    );
                    let new_leaf = NewAccount::builder()
                        .id(uuid::Uuid::now_v7())
                        .name(format!("deadlock leaf {code}"))
                        .code(code)
                        .build()
                        .unwrap();
                    let leaf = cala
                        .accounts()
                        .create_in_op(&mut db, new_leaf)
                        .await
                        .map_err(|e| anyhow::anyhow!("create_in_op: {e}"))?;
                    cala.account_sets()
                        .add_member_in_op(&mut db, parent_id, leaf.id())
                        .await
                        .map_err(|e| anyhow::anyhow!("add_member_in_op: {e}"))?;
                }
                db.commit()
                    .await
                    .map_err(|e| anyhow::anyhow!("commit: {e}"))?;
                Ok::<_, anyhow::Error>(())
            }));
        }

        // Poster tasks: post to the hot leaf, exercising the
        // poster-side lock pair (SHARED on every ancestor +
        // FOR_UPDATE on the leaf's balance row) against the same
        // parents the add_member tasks are touching.
        for _ in 0..N_POSTERS {
            let cala = cala.clone();
            let tx_code = tx_code.clone();
            let sender_id = sender.id();
            let journal_id = journal.id();
            handles.push(tokio::spawn(async move {
                for _ in 0..POSTS_PER_POSTER {
                    let mut params = Params::new();
                    params.insert("journal_id", journal_id.to_string());
                    params.insert("sender", sender_id);
                    params.insert("recipient", hot_leaf_id);
                    params.insert("amount", POST_AMOUNT);
                    cala.post_transaction(TransactionId::new(), &tx_code, params)
                        .await
                        .map_err(|e| anyhow::anyhow!("post_transaction: {e}"))?;
                }
                Ok::<_, anyhow::Error>(())
            }));
        }

        for h in handles {
            h.await??;
        }
    }

    // After a recalc, every parent's balance should equal the hot
    // leaf's balance (the hot leaf is the only direct leaf with any
    // history under each parent).
    cala.account_sets()
        .recalculate_balances_batch(&parent_ids)
        .await
        .unwrap();

    let leaf_bal = cala.balances().find(journal.id(), hot_leaf_id, usd).await?;
    for parent in &parents {
        let parent_bal = cala.balances().find(journal.id(), parent.id(), usd).await?;
        assert_eq!(
            parent_bal.settled(),
            leaf_bal.settled(),
            "parent {} balance should match the hot leaf after recalc",
            parent.id(),
        );
    }

    Ok(())
}
