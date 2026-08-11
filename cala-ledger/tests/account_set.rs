mod helpers;

use std::time::Duration;

use rand::distr::{Alphanumeric, SampleString};

use cala_ledger::{
    account::*, account_set::error::AccountSetError, account_set::*, tx_template::*, *,
};

#[tokio::test]
async fn errors_on_collision() -> anyhow::Result<()> {
    let pool = helpers::init_pool().await?;
    let mut jobs = helpers::init_jobs(pool.clone()).await?;
    let cala_config = CalaLedgerConfig::builder()
        .pool(pool)
        .exec_migrations(false)
        .build()?;
    let cala = CalaLedger::init(cala_config, &mut jobs).await?;

    let (one, two) = helpers::test_accounts();
    let one = cala.accounts().create(one).await.unwrap();
    let two = cala.accounts().create(two).await.unwrap();

    let new_journal = helpers::test_journal();
    let journal = cala.journals().create(new_journal).await.unwrap();

    let set_one = NewAccountSet::builder()
        .id(AccountSetId::new())
        .name("SET ONE")
        .journal_id(journal.id())
        .balance_rollup(BalanceRollup::Synchronous)
        .build()
        .unwrap();
    let set_one = cala.account_sets().create(set_one).await.unwrap();

    let set_two = NewAccountSet::builder()
        .id(AccountSetId::new())
        .name("SET TWO")
        .journal_id(journal.id())
        .balance_rollup(BalanceRollup::Synchronous)
        .build()
        .unwrap();
    let set_two = cala.account_sets().create(set_two).await.unwrap();

    let parent = NewAccountSet::builder()
        .id(AccountSetId::new())
        .name("parent")
        .journal_id(journal.id())
        .balance_rollup(BalanceRollup::Synchronous)
        .build()
        .unwrap();
    let parent = cala.account_sets().create(parent).await.unwrap();

    // Cannot add the same account twice
    let res = cala.account_sets().add_member(set_one.id(), one.id()).await;
    assert!(res.is_ok());
    let res = cala.account_sets().add_member(set_one.id(), one.id()).await;
    assert!(res.is_err());

    // Cannot add an account included in child
    let res = cala
        .account_sets()
        .add_member(parent.id(), set_one.id())
        .await;
    assert!(res.is_ok());
    let res = cala.account_sets().add_member(parent.id(), one.id()).await;
    assert!(res.is_err());

    let res = cala.account_sets().add_member(set_two.id(), two.id()).await;
    assert!(res.is_ok());
    let res = cala.account_sets().add_member(parent.id(), two.id()).await;
    assert!(res.is_ok());

    // Cannot add an account included in sibling
    let res = cala.account_sets().add_member(set_one.id(), two.id()).await;
    assert!(res.is_err());

    // remove one from set_one
    let res = cala
        .account_sets()
        .remove_member(set_one.id(), one.id())
        .await;
    assert!(res.is_ok());

    // can add one to parent set
    let res = cala.account_sets().add_member(parent.id(), one.id()).await;
    assert!(res.is_ok());

    Ok(())
}

#[tokio::test]
async fn errors_on_membership_cycle() -> anyhow::Result<()> {
    let pool = helpers::init_pool().await?;
    let mut jobs = helpers::init_jobs(pool.clone()).await?;
    let cala_config = CalaLedgerConfig::builder()
        .pool(pool)
        .exec_migrations(false)
        .build()?;
    let cala = CalaLedger::init(cala_config, &mut jobs).await?;

    let journal = cala
        .journals()
        .create(helpers::test_journal())
        .await
        .unwrap();

    let new_set = |name: &str| {
        NewAccountSet::builder()
            .id(AccountSetId::new())
            .name(name.to_string())
            .journal_id(journal.id())
            .balance_rollup(BalanceRollup::Synchronous)
            .build()
            .unwrap()
    };
    let set_a = cala.account_sets().create(new_set("SET A")).await.unwrap();
    let set_b = cala.account_sets().create(new_set("SET B")).await.unwrap();
    let set_c = cala.account_sets().create(new_set("SET C")).await.unwrap();

    // B becomes a member of A, C a member of B
    let res = cala.account_sets().add_member(set_a.id(), set_b.id()).await;
    assert!(res.is_ok());
    let res = cala.account_sets().add_member(set_b.id(), set_c.id()).await;
    assert!(res.is_ok());

    // A is an ancestor of B: adding A to B would close a cycle
    let res = cala.account_sets().add_member(set_b.id(), set_a.id()).await;
    assert!(matches!(
        res,
        Err(AccountSetError::MembershipCycleDetected { .. })
    ));

    // A is a (transitive) ancestor of C: adding A to C would close a cycle
    let res = cala.account_sets().add_member(set_c.id(), set_a.id()).await;
    assert!(matches!(
        res,
        Err(AccountSetError::MembershipCycleDetected { .. })
    ));

    // A set can never be its own member
    let res = cala.account_sets().add_member(set_a.id(), set_a.id()).await;
    assert!(res.is_err());

    // C is already transitively under A (via B): a direct A<-C edge is not
    // a cycle, but it would give C a second path to A — double membership,
    // rejected. (An earlier implementation allowed this edge; the old closure
    // only collided once accounts were involved, and the edge then made
    // any later account-add under C fail. Walk-only rejects it up front.)
    let res = cala.account_sets().add_member(set_a.id(), set_c.id()).await;
    assert!(matches!(res, Err(AccountSetError::MemberAlreadyAdded)));

    // Genuinely independent additions still work
    let set_d = cala.account_sets().create(new_set("SET D")).await.unwrap();
    let res = cala.account_sets().add_member(set_a.id(), set_d.id()).await;
    assert!(res.is_ok());

    Ok(())
}

#[tokio::test]
async fn errors_on_membership_depth_exceeded() -> anyhow::Result<()> {
    let pool = helpers::init_pool().await?;
    let mut jobs = helpers::init_jobs(pool.clone()).await?;
    let cala_config = CalaLedgerConfig::builder()
        .pool(pool)
        .exec_migrations(false)
        .build()?;
    let cala = CalaLedger::init(cala_config, &mut jobs).await?;

    let journal = cala
        .journals()
        .create(helpers::test_journal())
        .await
        .unwrap();

    // A chain of 18 sets: s0 <- s1 <- ... <- s17. Building it top-down,
    // edge s(i)->s(i+1) sits at depth i+1; MAX_MEMBERSHIP_DEPTH is 16.
    let mut sets = Vec::new();
    for i in 0..18 {
        let set = NewAccountSet::builder()
            .id(AccountSetId::new())
            .name(format!("depth-set-{i}"))
            .journal_id(journal.id())
            .balance_rollup(BalanceRollup::Synchronous)
            .build()
            .unwrap();
        sets.push(cala.account_sets().create(set).await.unwrap());
    }

    // 16 edges (s0->s1 .. s15->s16) reach the maximum depth and are allowed.
    for i in 0..16 {
        let res = cala
            .account_sets()
            .add_member(sets[i].id(), sets[i + 1].id())
            .await;
        assert!(res.is_ok(), "edge {i} within the depth cap must be allowed");
    }

    // The next edge (s16->s17) would make a 17-deep chain, past the cap.
    let res = cala
        .account_sets()
        .add_member(sets[16].id(), sets[17].id())
        .await;
    assert!(
        matches!(res, Err(AccountSetError::MembershipDepthExceeded { .. })),
        "an edge past MAX_MEMBERSHIP_DEPTH must be rejected"
    );

    Ok(())
}

/// Path uniqueness: an account may be contained in any given set via at
/// most one membership path. The old materialized closure enforced this
/// through its unique constraint; walk-only must enforce it explicitly.
#[tokio::test]
async fn errors_on_double_membership() -> anyhow::Result<()> {
    let pool = helpers::init_pool().await?;
    let mut jobs = helpers::init_jobs(pool.clone()).await?;
    let cala_config = CalaLedgerConfig::builder()
        .pool(pool)
        .exec_migrations(false)
        .build()?;
    let cala = CalaLedger::init(cala_config, &mut jobs).await?;

    let journal = cala
        .journals()
        .create(helpers::test_journal())
        .await
        .unwrap();

    let new_set = |name: &str| {
        NewAccountSet::builder()
            .id(AccountSetId::new())
            .name(name.to_string())
            .journal_id(journal.id())
            .balance_rollup(BalanceRollup::Synchronous)
            .build()
            .unwrap()
    };
    // grandparent <- {branch_a, branch_b}
    let grandparent = cala
        .account_sets()
        .create(new_set("DOUBLE GP"))
        .await
        .unwrap();
    let branch_a = cala
        .account_sets()
        .create(new_set("DOUBLE BRANCH A"))
        .await
        .unwrap();
    let branch_b = cala
        .account_sets()
        .create(new_set("DOUBLE BRANCH B"))
        .await
        .unwrap();
    cala.account_sets()
        .add_member(grandparent.id(), branch_a.id())
        .await
        .unwrap();
    cala.account_sets()
        .add_member(grandparent.id(), branch_b.id())
        .await
        .unwrap();

    // Diamond via shared ancestor: an account in branch_a would reach the
    // grandparent twice if also added to branch_b.
    let (acct, other) = helpers::test_accounts();
    let acct = cala.accounts().create(acct).await.unwrap();
    let other = cala.accounts().create(other).await.unwrap();
    cala.account_sets()
        .add_member(branch_a.id(), acct.id())
        .await
        .unwrap();
    let res = cala
        .account_sets()
        .add_member(branch_b.id(), acct.id())
        .await;
    assert!(matches!(res, Err(AccountSetError::MemberAlreadyAdded)));

    // Same rule inside one batch: two pairs giving one account two paths
    // to the grandparent must be rejected atomically.
    let res = cala
        .account_sets()
        .add_members(&[(branch_a.id(), other.id()), (branch_b.id(), other.id())])
        .await;
    assert!(matches!(res, Err(AccountSetError::MemberAlreadyAdded)));

    // Set-level diamond: a set under branch_a cannot also be attached
    // under branch_b (it — and every account below it — would reach the
    // grandparent twice), even while it has no accounts yet.
    let nested = cala
        .account_sets()
        .create(new_set("DOUBLE NESTED"))
        .await
        .unwrap();
    cala.account_sets()
        .add_member(branch_a.id(), nested.id())
        .await
        .unwrap();
    let res = cala
        .account_sets()
        .add_member(branch_b.id(), nested.id())
        .await;
    assert!(matches!(res, Err(AccountSetError::MemberAlreadyAdded)));

    // Descendant-path overlap, all sets empty: a *descendant* of the
    // member already reaches the target chain through an edge that
    // bypasses the member. A ⊃ B, B ⊃ D, X ⊃ D — attaching X under A
    // would give D two paths to A, so it must be rejected even though X
    // itself has no ancestors and no accounts exist anywhere yet.
    let set_a2 = cala
        .account_sets()
        .create(new_set("DOUBLE DESC A"))
        .await
        .unwrap();
    let set_b2 = cala
        .account_sets()
        .create(new_set("DOUBLE DESC B"))
        .await
        .unwrap();
    let set_d2 = cala
        .account_sets()
        .create(new_set("DOUBLE DESC D"))
        .await
        .unwrap();
    let set_x2 = cala
        .account_sets()
        .create(new_set("DOUBLE DESC X"))
        .await
        .unwrap();
    cala.account_sets()
        .add_member(set_a2.id(), set_b2.id())
        .await
        .unwrap();
    cala.account_sets()
        .add_member(set_b2.id(), set_d2.id())
        .await
        .unwrap();
    cala.account_sets()
        .add_member(set_x2.id(), set_d2.id())
        .await
        .unwrap();
    let res = cala
        .account_sets()
        .add_member(set_a2.id(), set_x2.id())
        .await;
    assert!(matches!(res, Err(AccountSetError::MemberAlreadyAdded)));

    // Subtree-account overlap: attaching a set whose accounts already live
    // under the target chain is rejected.
    let outside = cala
        .account_sets()
        .create(new_set("DOUBLE OUTSIDE"))
        .await
        .unwrap();
    let (overlap, _) = helpers::test_accounts();
    let overlap = cala.accounts().create(overlap).await.unwrap();
    cala.account_sets()
        .add_member(outside.id(), overlap.id())
        .await
        .unwrap();
    cala.account_sets()
        .add_member(grandparent.id(), overlap.id())
        .await
        .unwrap();
    let res = cala
        .account_sets()
        .add_member(branch_b.id(), outside.id())
        .await;
    assert!(matches!(res, Err(AccountSetError::MemberAlreadyAdded)));

    Ok(())
}

#[tokio::test]
async fn add_members_batch() -> anyhow::Result<()> {
    let btc: Currency = "BTC".parse().unwrap();

    let pool = helpers::init_pool().await?;
    let mut jobs = helpers::init_jobs(pool.clone()).await?;
    let cala_config = CalaLedgerConfig::builder()
        .pool(pool)
        .exec_migrations(false)
        .build()?;
    let cala = CalaLedger::init(cala_config, &mut jobs).await?;

    let journal = cala
        .journals()
        .create(helpers::test_journal())
        .await
        .unwrap();

    let (sender, recipient) = helpers::test_accounts();
    let sender_account = cala.accounts().create(sender).await.unwrap();
    let recipient_account = cala.accounts().create(recipient).await.unwrap();

    let tx_code = Alphanumeric.sample_string(&mut rand::rng(), 32);
    let new_template = helpers::currency_conversion_template(&tx_code);
    cala.tx_templates().create(new_template).await.unwrap();

    let new_set = |name: &str| {
        NewAccountSet::builder()
            .id(AccountSetId::new())
            .name(name.to_string())
            .journal_id(journal.id())
            .balance_rollup(BalanceRollup::Synchronous)
            .build()
            .unwrap()
    };
    let recipient_set = cala
        .account_sets()
        .create(new_set("Recipient Set"))
        .await
        .unwrap();
    let sender_set = cala
        .account_sets()
        .create(new_set("Sender Set"))
        .await
        .unwrap();
    let parent_set = cala.account_sets().create(new_set("Parent")).await.unwrap();

    cala.account_sets()
        .add_member(parent_set.id(), recipient_set.id())
        .await
        .unwrap();
    cala.account_sets()
        .add_member(parent_set.id(), sender_set.id())
        .await
        .unwrap();

    // Empty batch is a no-op.
    cala.account_sets().add_members(&[]).await.unwrap();

    // Attach both accounts in one batch call.
    cala.account_sets()
        .add_members(&[
            (recipient_set.id(), recipient_account.id()),
            (sender_set.id(), sender_account.id()),
        ])
        .await
        .unwrap();

    let mut params = Params::new();
    params.insert("journal_id", journal.id().to_string());
    params.insert("sender", sender_account.id());
    params.insert("recipient", recipient_account.id());
    cala.post_transaction(TransactionId::new(), &tx_code, params)
        .await
        .unwrap();

    // Rollups see the batch-attached members exactly as the
    // single-attach path would produce.
    let recipient_balance = cala
        .balances()
        .find(journal.id(), recipient_account.id(), btc)
        .await?;
    let recipient_set_balance = cala
        .balances()
        .find(journal.id(), recipient_set.id(), btc)
        .await?;
    assert_eq!(recipient_balance.settled(), recipient_set_balance.settled());
    assert_eq!(
        recipient_balance.details.entry_id,
        recipient_set_balance.details.entry_id
    );

    let sender_balance = cala
        .balances()
        .find(journal.id(), sender_account.id(), btc)
        .await?;
    let sender_set_balance = cala
        .balances()
        .find(journal.id(), sender_set.id(), btc)
        .await?;
    assert_eq!(sender_balance.settled(), sender_set_balance.settled());

    // The grandparent receives both sides of the same transaction, so
    // its settled balance is zero.
    let parent_balance = cala
        .balances()
        .find(journal.id(), parent_set.id(), btc)
        .await?;
    assert_eq!(parent_balance.settled(), rust_decimal::Decimal::ZERO);

    // Re-attaching an existing member errors (the account now has
    // balance history, so the batch no-history check fires first).
    let res = cala
        .account_sets()
        .add_members(&[(recipient_set.id(), recipient_account.id())])
        .await;
    assert!(res.is_err());

    // Unknown target set errors.
    let (unknown, _) = helpers::test_accounts();
    let unknown = cala.accounts().create(unknown).await.unwrap();
    let res = cala
        .account_sets()
        .add_members(&[(AccountSetId::new(), unknown.id())])
        .await;
    assert!(res.is_err());

    Ok(())
}

#[tokio::test]
async fn add_member_sets_batch() -> anyhow::Result<()> {
    let pool = helpers::init_isolated_pool().await?;
    let mut jobs = helpers::init_jobs(pool.clone()).await?;
    let cala_config = CalaLedgerConfig::builder()
        .pool(pool.clone())
        .exec_migrations(false)
        .build()?;
    let cala = CalaLedger::init(cala_config, &mut jobs).await?;

    let journal = cala.journals().create(helpers::test_journal()).await?;
    let new_set = |name: &str| {
        NewAccountSet::builder()
            .id(AccountSetId::new())
            .name(name)
            .journal_id(journal.id())
            .balance_rollup(BalanceRollup::Synchronous)
            .build()
            .unwrap()
    };
    let root = cala.account_sets().create(new_set("batch-root")).await?;
    let left = cala.account_sets().create(new_set("batch-left")).await?;
    let right = cala.account_sets().create(new_set("batch-right")).await?;
    let leaf = cala.account_sets().create(new_set("batch-leaf")).await?;

    cala.account_sets().add_member_sets(&[]).await?;
    let epoch_before: i64 = sqlx::query_scalar("SELECT epoch FROM cala_account_set_graph_epoch")
        .fetch_one(&pool)
        .await?;
    cala.account_sets()
        .add_member_sets(&[
            (root.id(), left.id()),
            (root.id(), right.id()),
            (left.id(), leaf.id()),
        ])
        .await?;
    let epoch_after: i64 = sqlx::query_scalar("SELECT epoch FROM cala_account_set_graph_epoch")
        .fetch_one(&pool)
        .await?;
    assert_eq!(epoch_after, epoch_before + 1);

    let parent_ids = [root.id(), left.id()];
    let member_ids = [left.id(), right.id(), leaf.id()];
    let edge_count: i64 = sqlx::query_scalar(
        r#"
        SELECT COUNT(*)
        FROM cala_account_set_member_account_sets
        WHERE account_set_id = ANY($1)
          AND member_account_set_id = ANY($2)
        "#,
    )
    .bind(&parent_ids[..])
    .bind(&member_ids[..])
    .fetch_one(&pool)
    .await?;
    assert_eq!(edge_count, 3);

    let event_count: i64 = sqlx::query_scalar(
        r#"
        SELECT COUNT(*)
        FROM cala_persistent_outbox_events
        WHERE payload->>'type' = 'account_set_member_created'
          AND (payload->>'account_set_id')::uuid = ANY($1)
        "#,
    )
    .bind(&parent_ids[..])
    .fetch_one(&pool)
    .await?;
    assert_eq!(event_count, 3, "the batch must publish one event per edge");

    Ok(())
}

#[tokio::test]
async fn add_member_sets_batch_rejects_interacting_edges_atomically() -> anyhow::Result<()> {
    let pool = helpers::init_pool().await?;
    let mut jobs = helpers::init_jobs(pool.clone()).await?;
    let cala_config = CalaLedgerConfig::builder()
        .pool(pool.clone())
        .exec_migrations(false)
        .build()?;
    let cala = CalaLedger::init(cala_config, &mut jobs).await?;

    let journal = cala.journals().create(helpers::test_journal()).await?;
    let new_set = |name: &str| {
        NewAccountSet::builder()
            .id(AccountSetId::new())
            .name(name)
            .journal_id(journal.id())
            .balance_rollup(BalanceRollup::Synchronous)
            .build()
            .unwrap()
    };
    let set_a = cala.account_sets().create(new_set("batch-cycle-a")).await?;
    let set_b = cala.account_sets().create(new_set("batch-cycle-b")).await?;

    let result = cala
        .account_sets()
        .add_member_sets(&[(set_a.id(), set_b.id()), (set_b.id(), set_a.id())])
        .await;
    assert!(matches!(
        result,
        Err(AccountSetError::MembershipCycleDetected { .. })
    ));

    let ids = [set_a.id(), set_b.id()];
    let edge_count: i64 = sqlx::query_scalar(
        r#"
        SELECT COUNT(*)
        FROM cala_account_set_member_account_sets
        WHERE account_set_id = ANY($1)
           OR member_account_set_id = ANY($1)
        "#,
    )
    .bind(&ids[..])
    .fetch_one(&pool)
    .await?;
    assert_eq!(edge_count, 0, "a rejected batch must not insert any edge");

    Ok(())
}

#[tokio::test]
async fn add_member_sets_batch_rejects_duplicate_paths_atomically() -> anyhow::Result<()> {
    let pool = helpers::init_pool().await?;
    let mut jobs = helpers::init_jobs(pool.clone()).await?;
    let cala_config = CalaLedgerConfig::builder()
        .pool(pool.clone())
        .exec_migrations(false)
        .build()?;
    let cala = CalaLedger::init(cala_config, &mut jobs).await?;

    let journal = cala.journals().create(helpers::test_journal()).await?;
    let new_set = |name: &str| {
        NewAccountSet::builder()
            .id(AccountSetId::new())
            .name(name)
            .journal_id(journal.id())
            .balance_rollup(BalanceRollup::Synchronous)
            .build()
            .unwrap()
    };
    let root = cala
        .account_sets()
        .create(new_set("batch-path-root"))
        .await?;
    let branch = cala
        .account_sets()
        .create(new_set("batch-path-branch"))
        .await?;
    let leaf = cala
        .account_sets()
        .create(new_set("batch-path-leaf"))
        .await?;

    let result = cala
        .account_sets()
        .add_member_sets(&[
            (root.id(), branch.id()),
            (branch.id(), leaf.id()),
            (root.id(), leaf.id()),
        ])
        .await;
    assert!(matches!(result, Err(AccountSetError::MemberAlreadyAdded)));

    let ids = [root.id(), branch.id(), leaf.id()];
    let edge_count: i64 = sqlx::query_scalar(
        r#"
        SELECT COUNT(*)
        FROM cala_account_set_member_account_sets
        WHERE account_set_id = ANY($1)
          AND member_account_set_id = ANY($1)
        "#,
    )
    .bind(&ids[..])
    .fetch_one(&pool)
    .await?;
    assert_eq!(edge_count, 0, "a rejected batch must not insert any edge");

    Ok(())
}

#[tokio::test]
async fn add_member_sets_batch_rejects_account_conflict_from_interacting_edges(
) -> anyhow::Result<()> {
    let pool = helpers::init_pool().await?;
    let mut jobs = helpers::init_jobs(pool.clone()).await?;
    let cala_config = CalaLedgerConfig::builder()
        .pool(pool.clone())
        .exec_migrations(false)
        .build()?;
    let cala = CalaLedger::init(cala_config, &mut jobs).await?;

    let journal = cala.journals().create(helpers::test_journal()).await?;
    let new_set = |name: &str| {
        NewAccountSet::builder()
            .id(AccountSetId::new())
            .name(name)
            .journal_id(journal.id())
            .balance_rollup(BalanceRollup::Synchronous)
            .build()
            .unwrap()
    };
    let root = cala
        .account_sets()
        .create(new_set("batch-account-root"))
        .await?;
    let branch = cala
        .account_sets()
        .create(new_set("batch-account-branch"))
        .await?;
    let leaf = cala
        .account_sets()
        .create(new_set("batch-account-leaf"))
        .await?;
    let deep_leaf = cala
        .account_sets()
        .create(new_set("batch-account-deep-leaf"))
        .await?;
    cala.account_sets()
        .add_member(leaf.id(), deep_leaf.id())
        .await?;

    let (account, _) = helpers::test_accounts();
    let account = cala.accounts().create(account).await?;
    cala.account_sets()
        .add_member(root.id(), account.id())
        .await?;
    cala.account_sets()
        .add_member(deep_leaf.id(), account.id())
        .await?;

    // The account below `deep_leaf` is selected by following both proposed
    // edges and the committed leaf -> deep_leaf edge in the final descendant
    // closure. Loading all memberships for that candidate then exposes its
    // separate direct membership in `root`.
    let result = cala
        .account_sets()
        .add_member_sets(&[(root.id(), branch.id()), (branch.id(), leaf.id())])
        .await;
    assert!(matches!(result, Err(AccountSetError::MemberAlreadyAdded)));

    let parent_ids = [root.id(), branch.id()];
    let member_ids = [branch.id(), leaf.id()];
    let edge_count: i64 = sqlx::query_scalar(
        r#"
        SELECT COUNT(*)
        FROM cala_account_set_member_account_sets
        WHERE account_set_id = ANY($1)
          AND member_account_set_id = ANY($2)
        "#,
    )
    .bind(&parent_ids[..])
    .bind(&member_ids[..])
    .fetch_one(&pool)
    .await?;
    assert_eq!(edge_count, 0, "a rejected batch must not insert any edge");

    Ok(())
}

#[tokio::test]
async fn add_member_sets_batch_rejects_a_duplicate_of_a_committed_edge() -> anyhow::Result<()> {
    let pool = helpers::init_pool().await?;
    let mut jobs = helpers::init_jobs(pool.clone()).await?;
    let cala_config = CalaLedgerConfig::builder()
        .pool(pool.clone())
        .exec_migrations(false)
        .build()?;
    let cala = CalaLedger::init(cala_config, &mut jobs).await?;

    let journal = cala.journals().create(helpers::test_journal()).await?;
    let new_set = |name: &str| {
        NewAccountSet::builder()
            .id(AccountSetId::new())
            .name(name)
            .journal_id(journal.id())
            .balance_rollup(BalanceRollup::Synchronous)
            .build()
            .unwrap()
    };
    let parent = cala
        .account_sets()
        .create(new_set("batch-dup-parent"))
        .await?;
    let child = cala
        .account_sets()
        .create(new_set("batch-dup-child"))
        .await?;
    cala.account_sets()
        .add_member(parent.id(), child.id())
        .await?;

    // Re-attaching an already-committed edge must be rejected before the
    // unique constraint fires, matching the single-edge path.
    let result = cala
        .account_sets()
        .add_member_sets(&[(parent.id(), child.id())])
        .await;
    assert!(matches!(result, Err(AccountSetError::MemberAlreadyAdded)));

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
    assert_eq!(edge_count, 1, "the committed edge must remain exactly once");

    Ok(())
}

#[tokio::test]
async fn add_member_sets_batch_rejects_a_path_through_committed_edges() -> anyhow::Result<()> {
    let pool = helpers::init_pool().await?;
    let mut jobs = helpers::init_jobs(pool.clone()).await?;
    let cala_config = CalaLedgerConfig::builder()
        .pool(pool.clone())
        .exec_migrations(false)
        .build()?;
    let cala = CalaLedger::init(cala_config, &mut jobs).await?;

    let journal = cala.journals().create(helpers::test_journal()).await?;
    let new_set = |name: &str| {
        NewAccountSet::builder()
            .id(AccountSetId::new())
            .name(name)
            .journal_id(journal.id())
            .balance_rollup(BalanceRollup::Synchronous)
            .build()
            .unwrap()
    };
    let root = cala
        .account_sets()
        .create(new_set("batch-committed-root"))
        .await?;
    let branch = cala
        .account_sets()
        .create(new_set("batch-committed-branch"))
        .await?;
    let leaf = cala
        .account_sets()
        .create(new_set("batch-committed-leaf"))
        .await?;
    // Commit root ⊃ leaf, then propose root ⊃ branch and branch ⊃ leaf.
    // The second proposed edge gives leaf a second path to root.
    cala.account_sets().add_member(root.id(), leaf.id()).await?;

    let result = cala
        .account_sets()
        .add_member_sets(&[(root.id(), branch.id()), (branch.id(), leaf.id())])
        .await;
    assert!(matches!(result, Err(AccountSetError::MemberAlreadyAdded)));

    // The batch must not insert its proposed edges; the committed edge remains.
    let proposed_count: i64 = sqlx::query_scalar(
        r#"
        SELECT COUNT(*)
        FROM cala_account_set_member_account_sets
        WHERE (account_set_id = $1 AND member_account_set_id = $2)
           OR (account_set_id = $2 AND member_account_set_id = $3)
        "#,
    )
    .bind(root.id())
    .bind(branch.id())
    .bind(leaf.id())
    .fetch_one(&pool)
    .await?;
    assert_eq!(
        proposed_count, 0,
        "a rejected batch must not insert any edge"
    );

    Ok(())
}

#[tokio::test]
async fn add_member_sets_batch_attributes_depth_overflow_through_existing_edges(
) -> anyhow::Result<()> {
    let pool = helpers::init_pool().await?;
    let mut jobs = helpers::init_jobs(pool.clone()).await?;
    let cala_config = CalaLedgerConfig::builder()
        .pool(pool.clone())
        .exec_migrations(false)
        .build()?;
    let cala = CalaLedger::init(cala_config, &mut jobs).await?;

    let journal = cala.journals().create(helpers::test_journal()).await?;
    let mut sets = Vec::new();
    for i in 0..18 {
        let set = NewAccountSet::builder()
            .id(AccountSetId::new())
            .name(format!("batch-existing-depth-{i}"))
            .journal_id(journal.id())
            .balance_rollup(BalanceRollup::Synchronous)
            .build()?;
        sets.push(cala.account_sets().create(set).await?);
    }
    // Commit a 10-chain, then propose 8 more edges to exceed depth 16.
    for pair in sets[0..10].windows(2) {
        cala.account_sets()
            .add_member(pair[0].id(), pair[1].id())
            .await?;
    }
    let proposed: Vec<_> = sets[9..18]
        .windows(2)
        .map(|pair| (pair[0].id(), pair[1].id()))
        .collect();

    let result = cala.account_sets().add_member_sets(&proposed).await;
    assert!(matches!(
        result,
        Err(AccountSetError::MembershipDepthExceeded {
            account_set_id,
            member_account_set_id,
            depth: 17,
            max: 16,
        }) if account_set_id == sets[16].id()
            && member_account_set_id == sets[17].id()
    ));

    let ids: Vec<_> = sets[9..18].iter().map(|set| set.id()).collect();
    let edge_count: i64 = sqlx::query_scalar(
        r#"
        SELECT COUNT(*)
        FROM cala_account_set_member_account_sets
        WHERE account_set_id = ANY($1)
        "#,
    )
    .bind(&ids)
    .fetch_one(&pool)
    .await?;
    assert_eq!(edge_count, 0, "a rejected batch must not insert any edge");

    Ok(())
}

#[tokio::test]
async fn add_member_sets_batch_matches_serial_add_member_set() -> anyhow::Result<()> {
    let pool = helpers::init_pool().await?;
    let mut jobs = helpers::init_jobs(pool.clone()).await?;
    let cala_config = CalaLedgerConfig::builder()
        .pool(pool.clone())
        .exec_migrations(false)
        .build()?;
    let cala = CalaLedger::init(cala_config, &mut jobs).await?;

    let journal = cala.journals().create(helpers::test_journal()).await?;
    let new_set = |name: &str| {
        NewAccountSet::builder()
            .id(AccountSetId::new())
            .name(name)
            .journal_id(journal.id())
            .balance_rollup(BalanceRollup::Synchronous)
            .build()
            .unwrap()
    };
    // Two identical trees: one attached in a batch, one edge at a time.
    let batch_sets: Vec<_> = (0..5)
        .map(|i| {
            cala.account_sets()
                .create(new_set(&format!("diff-batch-{i}")))
        })
        .collect();
    let batch_sets = futures::future::try_join_all(batch_sets).await?;
    let serial_sets: Vec<_> = (0..5)
        .map(|i| {
            cala.account_sets()
                .create(new_set(&format!("diff-serial-{i}")))
        })
        .collect();
    let serial_sets = futures::future::try_join_all(serial_sets).await?;

    let edges: Vec<_> = batch_sets
        .windows(2)
        .map(|pair| (pair[0].id(), pair[1].id()))
        .collect();
    cala.account_sets().add_member_sets(&edges).await?;
    for pair in serial_sets.windows(2) {
        cala.account_sets()
            .add_member(pair[0].id(), pair[1].id())
            .await?;
    }

    let batch_count: i64 = sqlx::query_scalar(
        r#"
        SELECT COUNT(*)
        FROM cala_account_set_member_account_sets
        WHERE account_set_id = ANY($1)
        "#,
    )
    .bind(batch_sets.iter().map(|set| set.id()).collect::<Vec<_>>())
    .fetch_one(&pool)
    .await?;
    let serial_count: i64 = sqlx::query_scalar(
        r#"
        SELECT COUNT(*)
        FROM cala_account_set_member_account_sets
        WHERE account_set_id = ANY($1)
        "#,
    )
    .bind(serial_sets.iter().map(|set| set.id()).collect::<Vec<_>>())
    .fetch_one(&pool)
    .await?;
    assert_eq!(batch_count, serial_count);
    assert_eq!(batch_count, 4);

    Ok(())
}

#[tokio::test]
async fn add_member_sets_batch_rejects_dense_duplicate_paths_without_path_explosion(
) -> anyhow::Result<()> {
    let pool = helpers::init_pool().await?;
    let mut jobs = helpers::init_jobs(pool.clone()).await?;
    let cala_config = CalaLedgerConfig::builder()
        .pool(pool)
        .exec_migrations(false)
        .build()?;
    let cala = CalaLedger::init(cala_config, &mut jobs).await?;

    let journal = cala.journals().create(helpers::test_journal()).await?;
    let mut sets = Vec::new();
    for i in 0..64 {
        let set = NewAccountSet::builder()
            .id(AccountSetId::new())
            .name(format!("batch-dense-{i}"))
            .journal_id(journal.id())
            .balance_rollup(BalanceRollup::Synchronous)
            .build()?;
        sets.push(cala.account_sets().create(set).await?);
    }
    let mut edges = Vec::new();
    for parent in 0..sets.len() {
        for child in (parent + 1)..sets.len() {
            edges.push((sets[parent].id(), sets[child].id()));
        }
    }

    let result = tokio::time::timeout(
        Duration::from_secs(5),
        cala.account_sets().add_member_sets(&edges),
    )
    .await
    .expect("dense invalid input must be rejected with bounded work");
    assert!(matches!(result, Err(AccountSetError::MemberAlreadyAdded)));

    Ok(())
}

#[tokio::test]
async fn add_member_sets_batch_rejects_depth_overflow_atomically() -> anyhow::Result<()> {
    let pool = helpers::init_pool().await?;
    let mut jobs = helpers::init_jobs(pool.clone()).await?;
    let cala_config = CalaLedgerConfig::builder()
        .pool(pool.clone())
        .exec_migrations(false)
        .build()?;
    let cala = CalaLedger::init(cala_config, &mut jobs).await?;

    let journal = cala.journals().create(helpers::test_journal()).await?;
    let mut sets = Vec::new();
    for i in 0..18 {
        let set = NewAccountSet::builder()
            .id(AccountSetId::new())
            .name(format!("batch-depth-{i}"))
            .journal_id(journal.id())
            .balance_rollup(BalanceRollup::Synchronous)
            .build()?;
        sets.push(cala.account_sets().create(set).await?);
    }
    let edges: Vec<_> = sets
        .windows(2)
        .map(|pair| (pair[0].id(), pair[1].id()))
        .collect();

    let result = cala.account_sets().add_member_sets(&edges).await;
    assert!(matches!(
        result,
        Err(AccountSetError::MembershipDepthExceeded {
            account_set_id,
            member_account_set_id,
            depth: 17,
            max: 16,
        }) if account_set_id == sets[16].id()
            && member_account_set_id == sets[17].id()
    ));

    let ids: Vec<_> = sets.iter().map(|set| set.id()).collect();
    let edge_count: i64 = sqlx::query_scalar(
        r#"
        SELECT COUNT(*)
        FROM cala_account_set_member_account_sets
        WHERE account_set_id = ANY($1)
        "#,
    )
    .bind(&ids)
    .fetch_one(&pool)
    .await?;
    assert_eq!(edge_count, 0, "a rejected batch must not insert any edge");

    Ok(())
}

#[tokio::test]
async fn add_member_sets_batch_rejects_journal_mismatch_atomically() -> anyhow::Result<()> {
    let pool = helpers::init_pool().await?;
    let mut jobs = helpers::init_jobs(pool.clone()).await?;
    let cala_config = CalaLedgerConfig::builder()
        .pool(pool.clone())
        .exec_migrations(false)
        .build()?;
    let cala = CalaLedger::init(cala_config, &mut jobs).await?;

    let journal_a = cala.journals().create(helpers::test_journal()).await?;
    let journal_b = cala.journals().create(helpers::test_journal()).await?;
    let new_set = |name: &str, journal_id| {
        NewAccountSet::builder()
            .id(AccountSetId::new())
            .name(name)
            .journal_id(journal_id)
            .balance_rollup(BalanceRollup::Synchronous)
            .build()
            .unwrap()
    };
    let parent = cala
        .account_sets()
        .create(new_set("batch-journal-parent", journal_a.id()))
        .await?;
    let valid_child = cala
        .account_sets()
        .create(new_set("batch-journal-valid", journal_a.id()))
        .await?;
    let invalid_child = cala
        .account_sets()
        .create(new_set("batch-journal-invalid", journal_b.id()))
        .await?;

    let result = cala
        .account_sets()
        .add_member_sets(&[
            (parent.id(), valid_child.id()),
            (parent.id(), invalid_child.id()),
        ])
        .await;
    assert!(matches!(result, Err(AccountSetError::JournalIdMismatch)));

    let edge_count: i64 = sqlx::query_scalar(
        r#"
        SELECT COUNT(*)
        FROM cala_account_set_member_account_sets
        WHERE account_set_id = $1
        "#,
    )
    .bind(parent.id())
    .fetch_one(&pool)
    .await?;
    assert_eq!(edge_count, 0, "a rejected batch must not insert any edge");

    Ok(())
}

#[tokio::test]
async fn add_member_sets_batch_rejects_member_history_atomically() -> anyhow::Result<()> {
    let pool = helpers::init_pool().await?;
    let mut jobs = helpers::init_jobs(pool.clone()).await?;
    let cala_config = CalaLedgerConfig::builder()
        .pool(pool.clone())
        .exec_migrations(false)
        .build()?;
    let cala = CalaLedger::init(cala_config, &mut jobs).await?;

    let journal = cala.journals().create(helpers::test_journal()).await?;
    let (sender, recipient) = helpers::test_accounts();
    let sender = cala.accounts().create(sender).await?;
    let recipient = cala.accounts().create(recipient).await?;
    let tx_code = Alphanumeric.sample_string(&mut rand::rng(), 32);
    cala.tx_templates()
        .create(helpers::currency_conversion_template(&tx_code))
        .await?;

    let new_set = |name: &str| {
        NewAccountSet::builder()
            .id(AccountSetId::new())
            .name(name)
            .journal_id(journal.id())
            .balance_rollup(BalanceRollup::Synchronous)
            .build()
            .unwrap()
    };
    let parent = cala
        .account_sets()
        .create(new_set("batch-history-parent"))
        .await?;
    let valid_child = cala
        .account_sets()
        .create(new_set("batch-history-valid"))
        .await?;
    let child_with_history = cala
        .account_sets()
        .create(new_set("batch-history-invalid"))
        .await?;
    cala.account_sets()
        .add_member(child_with_history.id(), recipient.id())
        .await?;

    let mut params = Params::new();
    params.insert("journal_id", journal.id().to_string());
    params.insert("sender", sender.id());
    params.insert("recipient", recipient.id());
    cala.post_transaction(TransactionId::new(), &tx_code, params)
        .await?;

    let result = cala
        .account_sets()
        .add_member_sets(&[
            (parent.id(), valid_child.id()),
            (parent.id(), child_with_history.id()),
        ])
        .await;
    assert!(matches!(
        result,
        Err(AccountSetError::MemberHasBalanceHistory { .. })
    ));

    let edge_count: i64 = sqlx::query_scalar(
        r#"
        SELECT COUNT(*)
        FROM cala_account_set_member_account_sets
        WHERE account_set_id = $1
        "#,
    )
    .bind(parent.id())
    .fetch_one(&pool)
    .await?;
    assert_eq!(edge_count, 0, "a rejected batch must not insert any edge");

    Ok(())
}

#[tokio::test]
async fn balances() -> anyhow::Result<()> {
    let btc: Currency = "BTC".parse().unwrap();

    let pool = helpers::init_pool().await?;
    let mut jobs = helpers::init_jobs(pool.clone()).await?;
    let cala_config = CalaLedgerConfig::builder()
        .pool(pool)
        .exec_migrations(false)
        .build()?;
    let cala = CalaLedger::init(cala_config, &mut jobs).await?;

    let new_journal = helpers::test_journal();
    let journal = cala.journals().create(new_journal).await.unwrap();

    let (sender, receiver) = helpers::test_accounts();
    let sender_account = cala.accounts().create(sender).await.unwrap();
    let recipient_account = cala.accounts().create(receiver).await.unwrap();

    let tx_code = Alphanumeric.sample_string(&mut rand::rng(), 32);
    let new_template = helpers::currency_conversion_template(&tx_code);
    cala.tx_templates().create(new_template).await.unwrap();

    let recipient_set = cala
        .account_sets()
        .create(
            NewAccountSet::builder()
                .id(AccountSetId::new())
                .name("Recipient Set")
                .journal_id(journal.id())
                .balance_rollup(BalanceRollup::Synchronous)
                .build()
                .unwrap(),
        )
        .await
        .unwrap();
    let sender_set = cala
        .account_sets()
        .create(
            NewAccountSet::builder()
                .id(AccountSetId::new())
                .name("Sender Set")
                .journal_id(journal.id())
                .balance_rollup(BalanceRollup::Synchronous)
                .build()
                .unwrap(),
        )
        .await
        .unwrap();
    let parent_set = cala
        .account_sets()
        .create(
            NewAccountSet::builder()
                .id(AccountSetId::new())
                .name("Parent")
                .journal_id(journal.id())
                .balance_rollup(BalanceRollup::Synchronous)
                .build()
                .unwrap(),
        )
        .await
        .unwrap();

    // Wire the hierarchy up *before* any posts so the no-history rule
    // is satisfied for every membership change.
    cala.account_sets()
        .add_member(recipient_set.id(), recipient_account.id())
        .await
        .unwrap();
    cala.account_sets()
        .add_member(sender_set.id(), sender_account.id())
        .await
        .unwrap();
    cala.account_sets()
        .add_member(parent_set.id(), recipient_set.id())
        .await
        .unwrap();
    cala.account_sets()
        .add_member(parent_set.id(), sender_set.id())
        .await
        .unwrap();

    let mut params = Params::new();
    params.insert("journal_id", journal.id().to_string());
    params.insert("sender", sender_account.id());
    params.insert("recipient", recipient_account.id());
    cala.post_transaction(TransactionId::new(), &tx_code, params)
        .await
        .unwrap();

    // Each direct parent fold-up matches its single member exactly.
    let recipient_balance = cala
        .balances()
        .find(journal.id(), recipient_account.id(), btc)
        .await?;
    let recipient_set_balance = cala
        .balances()
        .find(journal.id(), recipient_set.id(), btc)
        .await?;
    assert_eq!(recipient_balance.settled(), recipient_set_balance.settled());
    assert_eq!(
        recipient_balance.details.entry_id,
        recipient_set_balance.details.entry_id
    );

    let sender_balance = cala
        .balances()
        .find(journal.id(), sender_account.id(), btc)
        .await?;
    let sender_set_balance = cala
        .balances()
        .find(journal.id(), sender_set.id(), btc)
        .await?;
    assert_eq!(sender_balance.settled(), sender_set_balance.settled());
    assert_eq!(
        sender_balance.details.entry_id,
        sender_set_balance.details.entry_id
    );

    // The grandparent receives both sides of the same transaction, so
    // its settled balance is zero.
    let parent_balance = cala
        .balances()
        .find(journal.id(), parent_set.id(), btc)
        .await?;
    assert_eq!(parent_balance.settled(), rust_decimal::Decimal::ZERO);

    let query_args = es_entity::PaginatedQueryArgs {
        first: 2,
        after: None,
    };
    let ret = cala
        .entries()
        .list_for_account_set_id(
            recipient_set.id(),
            query_args,
            es_entity::ListDirection::Ascending,
        )
        .await?;

    assert!(!ret.entities.is_empty());
    Ok(())
}

#[tokio::test]
async fn account_set_update() -> anyhow::Result<()> {
    let pool = helpers::init_pool().await?;
    let mut jobs = helpers::init_jobs(pool.clone()).await?;
    let cala_config = CalaLedgerConfig::builder()
        .pool(pool)
        .exec_migrations(false)
        .build()?;
    let cala = CalaLedger::init(cala_config, &mut jobs).await?;

    let new_journal = helpers::test_journal();
    let journal = cala.journals().create(new_journal).await.unwrap();

    // create account set
    let initial_name = Alphanumeric.sample_string(&mut rand::rng(), 32);
    let new_account_set = NewAccountSet::builder()
        .id(AccountSetId::new())
        .name(initial_name.clone())
        .journal_id(journal.id())
        .balance_rollup(BalanceRollup::Synchronous)
        .build()?;

    let mut account_set = cala.account_sets().create(new_account_set).await?;
    assert_eq!(initial_name, account_set.values().name);

    // update account set name and description
    let updated_name = Alphanumeric.sample_string(&mut rand::rng(), 32);
    let mut builder = AccountSetUpdate::default();
    builder.name(updated_name.clone()).build()?;
    if account_set.update(builder).did_execute() {
        cala.account_sets().persist(&mut account_set).await?;
    }
    assert_eq!(updated_name, account_set.values().name);
    Ok(())
}

#[tokio::test]
async fn members_pagination() -> anyhow::Result<()> {
    let pool = helpers::init_pool().await?;
    let mut jobs = helpers::init_jobs(pool.clone()).await?;
    let cala_config = CalaLedgerConfig::builder()
        .pool(pool)
        .exec_migrations(false)
        .build()?;
    let cala = CalaLedger::init(cala_config, &mut jobs).await?;
    let new_journal = helpers::test_journal();
    let journal = cala.journals().create(new_journal).await.unwrap();

    let (one, two) = helpers::test_accounts();
    let account_one = cala.accounts().create(one).await.unwrap();
    let account_two = cala.accounts().create(two).await.unwrap();

    let set_one = NewAccountSet::builder()
        .id(AccountSetId::new())
        .name("SET ONE")
        .journal_id(journal.id())
        .balance_rollup(BalanceRollup::Synchronous)
        .build()
        .unwrap();
    let set_one = cala.account_sets().create(set_one).await.unwrap();
    let set_two = NewAccountSet::builder()
        .id(AccountSetId::new())
        .name("SET TWO")
        .journal_id(journal.id())
        .balance_rollup(BalanceRollup::Synchronous)
        .build()
        .unwrap();
    let set_two = cala.account_sets().create(set_two).await.unwrap();

    let parent = NewAccountSet::builder()
        .id(AccountSetId::new())
        .name("parent")
        .journal_id(journal.id())
        .balance_rollup(BalanceRollup::Synchronous)
        .build()
        .unwrap();
    let parent = cala.account_sets().create(parent).await.unwrap();

    cala.account_sets()
        .add_member(parent.id(), account_two.id())
        .await
        .unwrap();

    cala.account_sets()
        .add_member(parent.id(), set_one.id())
        .await
        .unwrap();

    cala.account_sets()
        .add_member(parent.id(), account_one.id())
        .await
        .unwrap();

    cala.account_sets()
        .add_member(parent.id(), set_two.id())
        .await
        .unwrap();

    let query_args = es_entity::PaginatedQueryArgs {
        first: 2,
        after: None,
    };

    let ret = cala
        .account_sets()
        .list_members_by_created_at(parent.id(), query_args)
        .await?;

    assert_eq!(ret.entities.len(), 2);
    assert!(ret.has_next_page);
    assert_eq!(
        ret.entities[0].id.clone(),
        AccountSetMemberId::from(set_two.id())
    );
    assert_eq!(
        ret.entities[1].id.clone(),
        AccountSetMemberId::from(account_one.id())
    );

    let query_args = es_entity::PaginatedQueryArgs {
        first: 2,
        after: Some(AccountSetMemberByCreatedAtCursor::from(&ret.entities[0])),
    };

    let ret = cala
        .account_sets()
        .list_members_by_created_at(parent.id(), query_args)
        .await?;
    assert_eq!(ret.entities.len(), 2);
    assert!(ret.has_next_page);
    assert_eq!(
        ret.entities[0].id.clone(),
        AccountSetMemberId::from(account_one.id())
    );
    assert_eq!(
        ret.entities[1].id.clone(),
        AccountSetMemberId::from(set_one.id())
    );

    let query_args = es_entity::PaginatedQueryArgs {
        first: 2,
        after: Some(AccountSetMemberByCreatedAtCursor::from(&ret.entities[1])),
    };

    let ret = cala
        .account_sets()
        .list_members_by_created_at(parent.id(), query_args)
        .await?;
    assert_eq!(ret.entities.len(), 1);
    assert!(!ret.has_next_page);
    assert_eq!(
        ret.entities[0].id.clone(),
        AccountSetMemberId::from(account_two.id())
    );

    Ok(())
}

#[tokio::test]
async fn list_members_by_external_id() -> anyhow::Result<()> {
    let pool = helpers::init_pool().await?;
    let mut jobs = helpers::init_jobs(pool.clone()).await?;
    let cala = CalaLedger::init(
        CalaLedgerConfig::builder()
            .pool(pool)
            .exec_migrations(false)
            .build()?,
        &mut jobs,
    )
    .await?;

    let journal = cala.journals().create(helpers::test_journal()).await?;
    let parent = cala
        .account_sets()
        .create(
            NewAccountSet::builder()
                .id(AccountSetId::new())
                .name("Parent Set")
                .journal_id(journal.id())
                .balance_rollup(BalanceRollup::Synchronous)
                .build()?,
        )
        .await?;

    let random = Alphanumeric.sample_string(&mut rand::rng(), 8);

    let account_ids = [
        cala.accounts()
            .create(
                NewAccount::builder()
                    .id(AccountId::new())
                    .name(Alphanumeric.sample_string(&mut rand::rng(), 8))
                    .code(Alphanumeric.sample_string(&mut rand::rng(), 8))
                    .external_id(format!("a-{random}"))
                    .build()?,
            )
            .await?,
        cala.accounts()
            .create(
                NewAccount::builder()
                    .id(AccountId::new())
                    .name(Alphanumeric.sample_string(&mut rand::rng(), 8))
                    .code(Alphanumeric.sample_string(&mut rand::rng(), 8))
                    .external_id(format!("z-{random}"))
                    .build()?,
            )
            .await?,
        cala.accounts()
            .create(
                NewAccount::builder()
                    .id(AccountId::new())
                    .name(Alphanumeric.sample_string(&mut rand::rng(), 8))
                    .code(Alphanumeric.sample_string(&mut rand::rng(), 8))
                    .build()?,
            )
            .await?,
    ];

    for account in &account_ids {
        cala.account_sets()
            .add_member(parent.id(), account.id())
            .await?;
    }

    let query_args = es_entity::PaginatedQueryArgs {
        first: 1,
        after: None,
    };
    let ret = cala
        .account_sets()
        .list_members_by_external_id(parent.id(), query_args)
        .await?;
    assert_eq!(ret.entities[0].external_id, Some(format!("a-{random}")));

    let query_args = es_entity::PaginatedQueryArgs {
        first: 1,
        after: Some(AccountSetMemberByExternalIdCursor::from(&ret.entities[0])),
    };
    let ret = cala
        .account_sets()
        .list_members_by_external_id(parent.id(), query_args)
        .await?;
    assert_eq!(ret.entities[0].external_id, Some(format!("z-{random}")));

    let query_args = es_entity::PaginatedQueryArgs {
        first: 1,
        after: Some(AccountSetMemberByExternalIdCursor::from(&ret.entities[0])),
    };
    let ret = cala
        .account_sets()
        .list_members_by_external_id(parent.id(), query_args)
        .await?;
    assert_eq!(ret.entities[0].external_id, None);

    Ok(())
}

#[tokio::test]
async fn add_member_errors_when_member_has_history() -> anyhow::Result<()> {
    let pool = helpers::init_pool().await?;
    let mut jobs = helpers::init_jobs(pool.clone()).await?;
    let cala_config = CalaLedgerConfig::builder()
        .pool(pool)
        .exec_migrations(false)
        .build()?;
    let cala = CalaLedger::init(cala_config, &mut jobs).await?;

    let journal = cala
        .journals()
        .create(helpers::test_journal())
        .await
        .unwrap();

    let (sender, recipient) = helpers::test_accounts();
    let sender = cala.accounts().create(sender).await.unwrap();
    let recipient = cala.accounts().create(recipient).await.unwrap();

    let tx_code = Alphanumeric.sample_string(&mut rand::rng(), 32);
    cala.tx_templates()
        .create(helpers::currency_conversion_template(&tx_code))
        .await
        .unwrap();

    // Post once so the recipient has history before any membership change.
    let mut params = Params::new();
    params.insert("journal_id", journal.id().to_string());
    params.insert("sender", sender.id());
    params.insert("recipient", recipient.id());
    cala.post_transaction(TransactionId::new(), &tx_code, params)
        .await
        .unwrap();

    let target = cala
        .account_sets()
        .create(
            NewAccountSet::builder()
                .id(AccountSetId::new())
                .name("Target")
                .journal_id(journal.id())
                .balance_rollup(BalanceRollup::Synchronous)
                .build()
                .unwrap(),
        )
        .await
        .unwrap();

    let err = cala
        .account_sets()
        .add_member(target.id(), recipient.id())
        .await
        .err()
        .expect("add_member should fail when the member has balance history");

    match err {
        AccountSetError::MemberHasBalanceHistory {
            account_set_id,
            member_id,
        } => {
            assert_eq!(account_set_id, target.id());
            assert_eq!(member_id, recipient.id());
        }
        other => panic!("expected MemberHasBalanceHistory, got {other}"),
    }

    // Adding a fresh account with no history is still allowed.
    let fresh = cala
        .accounts()
        .create(
            NewAccount::builder()
                .id(uuid::Uuid::now_v7())
                .name("Fresh")
                .code(Alphanumeric.sample_string(&mut rand::rng(), 32))
                .build()
                .unwrap(),
        )
        .await
        .unwrap();
    cala.account_sets()
        .add_member(target.id(), fresh.id())
        .await
        .unwrap();

    Ok(())
}

/// `remove_member` must reject members that have any
/// `cala_balance_history` rows: there is no safe way to back the member's
/// past contribution out of the parent set's running balance.
#[tokio::test]
async fn remove_member_errors_when_member_has_history() -> anyhow::Result<()> {
    let pool = helpers::init_pool().await?;
    let mut jobs = helpers::init_jobs(pool.clone()).await?;
    let cala_config = CalaLedgerConfig::builder()
        .pool(pool)
        .exec_migrations(false)
        .build()?;
    let cala = CalaLedger::init(cala_config, &mut jobs).await?;

    let journal = cala
        .journals()
        .create(helpers::test_journal())
        .await
        .unwrap();

    let (sender, recipient) = helpers::test_accounts();
    let sender = cala.accounts().create(sender).await.unwrap();
    let recipient = cala.accounts().create(recipient).await.unwrap();

    let tx_code = Alphanumeric.sample_string(&mut rand::rng(), 32);
    cala.tx_templates()
        .create(helpers::currency_conversion_template(&tx_code))
        .await
        .unwrap();

    let target = cala
        .account_sets()
        .create(
            NewAccountSet::builder()
                .id(AccountSetId::new())
                .name("Target")
                .journal_id(journal.id())
                .balance_rollup(BalanceRollup::Synchronous)
                .build()
                .unwrap(),
        )
        .await
        .unwrap();

    // Add the recipient *before* it has any history (allowed) — then post
    // to it so that subsequent removal becomes a forbidden operation.
    cala.account_sets()
        .add_member(target.id(), recipient.id())
        .await
        .unwrap();

    let mut params = Params::new();
    params.insert("journal_id", journal.id().to_string());
    params.insert("sender", sender.id());
    params.insert("recipient", recipient.id());
    cala.post_transaction(TransactionId::new(), &tx_code, params)
        .await
        .unwrap();

    let err = cala
        .account_sets()
        .remove_member(target.id(), recipient.id())
        .await
        .err()
        .expect("remove_member should fail when the member has balance history");

    match err {
        AccountSetError::MemberHasBalanceHistory {
            account_set_id,
            member_id,
        } => {
            assert_eq!(account_set_id, target.id());
            assert_eq!(member_id, recipient.id());
        }
        other => panic!("expected MemberHasBalanceHistory, got {other}"),
    }

    Ok(())
}

/// The double-membership check's in-memory path (epoch-matched set-graph
/// cache snapshot) must enforce exactly what the SQL walk enforces. The
/// hierarchy here is fully committed BEFORE the cache warms, so every
/// attach below runs against a matching epoch and resolves in memory;
/// the same scenarios in `errors_on_double_membership` run against a
/// cold or epoch-bumped cache and exercise the fallback walk.
#[tokio::test]
async fn double_membership_memory_path_parity() -> anyhow::Result<()> {
    let pool = helpers::init_pool().await?;
    let mut jobs = helpers::init_jobs(pool.clone()).await?;
    let cala_config = CalaLedgerConfig::builder()
        .pool(pool)
        .exec_migrations(false)
        .build()?;
    let cala = CalaLedger::init(cala_config, &mut jobs).await?;

    let journal = cala
        .journals()
        .create(helpers::test_journal())
        .await
        .unwrap();
    let new_set = |name: &str| {
        NewAccountSet::builder()
            .id(AccountSetId::new())
            .name(name.to_string())
            .journal_id(journal.id())
            .balance_rollup(BalanceRollup::Synchronous)
            .build()
            .unwrap()
    };
    // gp <- {branch_a, branch_b}: the shared ancestor that turns a
    // second attach into a second path.
    let gp = cala.account_sets().create(new_set("MEM GP")).await.unwrap();
    let branch_a = cala
        .account_sets()
        .create(new_set("MEM BRANCH A"))
        .await
        .unwrap();
    let branch_b = cala
        .account_sets()
        .create(new_set("MEM BRANCH B"))
        .await
        .unwrap();
    cala.account_sets()
        .add_member(gp.id(), branch_a.id())
        .await
        .unwrap();
    cala.account_sets()
        .add_member(gp.id(), branch_b.id())
        .await
        .unwrap();

    // Warm the cache: the first attach's check runs against a stale
    // epoch (fallback walk) and triggers the installing refresh.
    let (warmup, acct) = helpers::test_accounts();
    let warmup = cala.accounts().create(warmup).await.unwrap();
    let acct = cala.accounts().create(acct).await.unwrap();
    cala.account_sets()
        .add_member(branch_a.id(), warmup.id())
        .await
        .unwrap();
    tokio::time::sleep(std::time::Duration::from_secs(1)).await;

    // Warm single-pair path: legit attach passes, the diamond is
    // rejected.
    cala.account_sets()
        .add_member(branch_a.id(), acct.id())
        .await
        .unwrap();
    let res = cala
        .account_sets()
        .add_member(branch_b.id(), acct.id())
        .await;
    assert!(matches!(res, Err(AccountSetError::MemberAlreadyAdded)));

    // Warm batch path: cross-pair conflict rejected, distinct accounts
    // pass.
    let (other, third) = helpers::test_accounts();
    let other = cala.accounts().create(other).await.unwrap();
    let third = cala.accounts().create(third).await.unwrap();
    let res = cala
        .account_sets()
        .add_members(&[(branch_a.id(), other.id()), (branch_b.id(), other.id())])
        .await;
    assert!(matches!(res, Err(AccountSetError::MemberAlreadyAdded)));
    cala.account_sets()
        .add_members(&[(branch_a.id(), other.id()), (branch_b.id(), third.id())])
        .await
        .unwrap();

    // In-op probe visibility: the first attach's uncommitted row must
    // count as an existing path for the second attach in the same op.
    let (fourth, fifth) = helpers::test_accounts();
    let fourth = cala.accounts().create(fourth).await.unwrap();
    let fifth = cala.accounts().create(fifth).await.unwrap();
    let mut op = cala.begin_operation().await?;
    cala.account_sets()
        .add_member_in_op(&mut op, branch_a.id(), fourth.id())
        .await
        .unwrap();
    let res = cala
        .account_sets()
        .add_member_in_op(&mut op, branch_b.id(), fourth.id())
        .await;
    assert!(matches!(res, Err(AccountSetError::MemberAlreadyAdded)));
    drop(op);

    // Duplicate direct edge in one op: the second identical attach is
    // rejected before the unique constraint would fire.
    let mut op = cala.begin_operation().await?;
    cala.account_sets()
        .add_member_in_op(&mut op, branch_a.id(), fifth.id())
        .await
        .unwrap();
    let res = cala
        .account_sets()
        .add_member_in_op(&mut op, branch_a.id(), fifth.id())
        .await;
    assert!(matches!(res, Err(AccountSetError::MemberAlreadyAdded)));
    drop(op);

    Ok(())
}

/// Same-op structure interactions with the in-memory check: a set
/// created inside the op (no epoch bump) is unknown to the warm
/// snapshot and resolves via the op-local overlay; a set->set edge
/// added inside the op bumps the epoch in-op and pushes the check onto
/// the SQL fallback, which must see the op's own uncommitted edge.
#[tokio::test]
async fn double_membership_check_same_op_structure() -> anyhow::Result<()> {
    let pool = helpers::init_pool().await?;
    let mut jobs = helpers::init_jobs(pool.clone()).await?;
    let cala_config = CalaLedgerConfig::builder()
        .pool(pool)
        .exec_migrations(false)
        .build()?;
    let cala = CalaLedger::init(cala_config, &mut jobs).await?;

    let journal = cala
        .journals()
        .create(helpers::test_journal())
        .await
        .unwrap();
    let new_set = |name: &str| {
        NewAccountSet::builder()
            .id(AccountSetId::new())
            .name(name.to_string())
            .journal_id(journal.id())
            .balance_rollup(BalanceRollup::Synchronous)
            .build()
            .unwrap()
    };
    let gp = cala
        .account_sets()
        .create(new_set("SAMEOP GP"))
        .await
        .unwrap();
    let left = cala
        .account_sets()
        .create(new_set("SAMEOP LEFT"))
        .await
        .unwrap();
    let right = cala
        .account_sets()
        .create(new_set("SAMEOP RIGHT"))
        .await
        .unwrap();
    cala.account_sets()
        .add_member(gp.id(), left.id())
        .await
        .unwrap();
    cala.account_sets()
        .add_member(gp.id(), right.id())
        .await
        .unwrap();

    let (warmup, acct_a) = helpers::test_accounts();
    let warmup = cala.accounts().create(warmup).await.unwrap();
    let acct_a = cala.accounts().create(acct_a).await.unwrap();
    cala.account_sets()
        .add_member(left.id(), warmup.id())
        .await
        .unwrap();
    tokio::time::sleep(std::time::Duration::from_secs(1)).await;

    // Supplement path: create a set and attach an account to it in one
    // op — the set is unknown to the warm snapshot and no epoch bump
    // happens.
    let mut op = cala.begin_operation().await?;
    let fresh_set = cala
        .account_sets()
        .create_in_op(&mut op, new_set("SAMEOP FRESH"))
        .await
        .unwrap();
    cala.account_sets()
        .add_member_in_op(&mut op, fresh_set.id(), acct_a.id())
        .await
        .unwrap();
    op.commit().await?;

    // The committed membership is a countable existing path (the fresh
    // set still resolves through the overlay until the next refresh).
    let res = cala
        .account_sets()
        .add_member(fresh_set.id(), acct_a.id())
        .await;
    assert!(matches!(res, Err(AccountSetError::MemberAlreadyAdded)));

    // Epoch-bump fallback: attach a new set under `left` (bumps the
    // epoch in-op), give an account a path through it, then try a second
    // path through `right` — the fallback walk must reject using the
    // op's own uncommitted edge.
    let sub = cala
        .account_sets()
        .create(new_set("SAMEOP SUB"))
        .await
        .unwrap();
    let (acct_b, _) = helpers::test_accounts();
    let acct_b = cala.accounts().create(acct_b).await.unwrap();
    let mut op = cala.begin_operation().await?;
    cala.account_sets()
        .add_member_in_op(&mut op, left.id(), sub.id())
        .await
        .unwrap();
    cala.account_sets()
        .add_member_in_op(&mut op, sub.id(), acct_b.id())
        .await
        .unwrap();
    let res = cala
        .account_sets()
        .add_member_in_op(&mut op, right.id(), acct_b.id())
        .await;
    assert!(matches!(res, Err(AccountSetError::MemberAlreadyAdded)));
    drop(op);

    Ok(())
}
