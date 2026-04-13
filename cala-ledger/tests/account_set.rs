mod helpers;

use rand::distr::{Alphanumeric, SampleString};

use cala_ledger::{
    account::*, account_set::error::AccountSetError, account_set::*, tx_template::*, *,
};

#[tokio::test]
async fn errors_on_collision() -> anyhow::Result<()> {
    let pool = helpers::init_pool().await?;
    let cala_config = CalaLedgerConfig::builder()
        .pool(pool)
        .exec_migrations(false)
        .build()?;
    let cala = CalaLedger::init(cala_config).await?;

    let (one, two) = helpers::test_accounts();
    let one = cala.accounts().create(one).await.unwrap();
    let two = cala.accounts().create(two).await.unwrap();

    let new_journal = helpers::test_journal();
    let journal = cala.journals().create(new_journal).await.unwrap();

    let set_one = NewAccountSet::builder()
        .id(AccountSetId::new())
        .name("SET ONE")
        .journal_id(journal.id())
        .build()
        .unwrap();
    let set_one = cala.account_sets().create(set_one).await.unwrap();

    let set_two = NewAccountSet::builder()
        .id(AccountSetId::new())
        .name("SET TWO")
        .journal_id(journal.id())
        .build()
        .unwrap();
    let set_two = cala.account_sets().create(set_two).await.unwrap();

    let parent = NewAccountSet::builder()
        .id(AccountSetId::new())
        .name("parent")
        .journal_id(journal.id())
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
async fn balances() -> anyhow::Result<()> {
    let btc: Currency = "BTC".parse().unwrap();

    let pool = helpers::init_pool().await?;
    let cala_config = CalaLedgerConfig::builder()
        .pool(pool)
        .exec_migrations(false)
        .build()?;
    let cala = CalaLedger::init(cala_config).await?;

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
    let cala_config = CalaLedgerConfig::builder()
        .pool(pool)
        .exec_migrations(false)
        .build()?;
    let cala = CalaLedger::init(cala_config).await?;

    let new_journal = helpers::test_journal();
    let journal = cala.journals().create(new_journal).await.unwrap();

    // create account set
    let initial_name = Alphanumeric.sample_string(&mut rand::rng(), 32);
    let new_account_set = NewAccountSet::builder()
        .id(AccountSetId::new())
        .name(initial_name.clone())
        .journal_id(journal.id())
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
    let cala_config = CalaLedgerConfig::builder()
        .pool(pool)
        .exec_migrations(false)
        .build()?;
    let cala = CalaLedger::init(cala_config).await?;
    let new_journal = helpers::test_journal();
    let journal = cala.journals().create(new_journal).await.unwrap();

    let (one, two) = helpers::test_accounts();
    let account_one = cala.accounts().create(one).await.unwrap();
    let account_two = cala.accounts().create(two).await.unwrap();

    let set_one = NewAccountSet::builder()
        .id(AccountSetId::new())
        .name("SET ONE")
        .journal_id(journal.id())
        .build()
        .unwrap();
    let set_one = cala.account_sets().create(set_one).await.unwrap();
    let set_two = NewAccountSet::builder()
        .id(AccountSetId::new())
        .name("SET TWO")
        .journal_id(journal.id())
        .build()
        .unwrap();
    let set_two = cala.account_sets().create(set_two).await.unwrap();

    let parent = NewAccountSet::builder()
        .id(AccountSetId::new())
        .name("parent")
        .journal_id(journal.id())
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
    let cala = CalaLedger::init(
        CalaLedgerConfig::builder()
            .pool(pool)
            .exec_migrations(false)
            .build()?,
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
async fn eventually_consistent_balances() -> anyhow::Result<()> {
    use cala_ledger::balance::BalanceSnapshot;
    use sqlx::Row as _;

    let btc: Currency = "BTC".parse().unwrap();

    let pool = helpers::init_pool().await?;
    let cala_config = CalaLedgerConfig::builder()
        .pool(pool.clone())
        .exec_migrations(false)
        .build()?;
    let cala = CalaLedger::init(cala_config).await?;

    let journal = cala
        .journals()
        .create(helpers::test_journal())
        .await
        .unwrap();

    let (sender, receiver) = helpers::test_accounts();
    let sender_account = cala.accounts().create(sender).await.unwrap();
    let recipient_account = cala.accounts().create(receiver).await.unwrap();

    let tx_code = Alphanumeric.sample_string(&mut rand::rng(), 32);
    cala.tx_templates()
        .create(helpers::currency_conversion_template(&tx_code))
        .await
        .unwrap();

    // Create inline set for reference comparison
    let inline_set = NewAccountSet::builder()
        .id(AccountSetId::new())
        .name("Inline Set")
        .journal_id(journal.id())
        .build()
        .unwrap();
    let inline_set = cala.account_sets().create(inline_set).await.unwrap();

    // Create eventually_consistent set
    let ec_set = NewAccountSet::builder()
        .id(AccountSetId::new())
        .name("EC Set")
        .journal_id(journal.id())
        .eventually_consistent(true)
        .build()
        .unwrap();
    let ec_set = cala.account_sets().create(ec_set).await.unwrap();

    // Add recipient to both sets (no existing balance → no-op entries)
    cala.account_sets()
        .add_member(inline_set.id(), recipient_account.id())
        .await
        .unwrap();
    cala.account_sets()
        .add_member(ec_set.id(), recipient_account.id())
        .await
        .unwrap();

    // --- First batch: post 2 transactions ---
    for _ in 0..2 {
        let mut params = Params::new();
        params.insert("journal_id", journal.id().to_string());
        params.insert("sender", sender_account.id());
        params.insert("recipient", recipient_account.id());
        cala.post_transaction(TransactionId::new(), &tx_code, params)
            .await
            .unwrap();
    }

    // EC set should have NO balance inline
    assert!(
        cala.balances()
            .find(journal.id(), ec_set.id(), btc)
            .await
            .is_err(),
        "EC set should not have inline balance"
    );

    // First recalculate — processes all member history from scratch
    cala.account_sets()
        .recalculate_balances(ec_set.id())
        .await
        .unwrap();

    // Verify EC matches inline after first batch
    let inline_bal = cala
        .balances()
        .find(journal.id(), inline_set.id(), btc)
        .await?;
    let ec_bal = cala.balances().find(journal.id(), ec_set.id(), btc).await?;
    assert_eq!(inline_bal.settled(), ec_bal.settled());
    assert_eq!(inline_bal.details.version, ec_bal.details.version);

    // --- Second batch: post 2 more transactions ---
    for _ in 0..2 {
        let mut params = Params::new();
        params.insert("journal_id", journal.id().to_string());
        params.insert("sender", sender_account.id());
        params.insert("recipient", recipient_account.id());
        cala.post_transaction(TransactionId::new(), &tx_code, params)
            .await
            .unwrap();
    }

    // Second recalculate — incremental, only processes new changes
    cala.account_sets()
        .recalculate_balances(ec_set.id())
        .await
        .unwrap();

    // Verify EC matches inline after second batch
    let inline_bal_2 = cala
        .balances()
        .find(journal.id(), inline_set.id(), btc)
        .await?;
    let ec_bal_2 = cala.balances().find(journal.id(), ec_set.id(), btc).await?;
    assert_eq!(inline_bal_2.settled(), ec_bal_2.settled());
    assert_eq!(inline_bal_2.details.version, ec_bal_2.details.version);

    // Verify balance_history counts match across ALL currencies
    let inline_account_id = AccountId::from(&inline_set.id());
    let ec_account_id = AccountId::from(&ec_set.id());

    let inline_count: i64 = sqlx::query(
        "SELECT COUNT(*) as cnt FROM cala_balance_history WHERE account_id = $1 AND journal_id = $2",
    )
    .bind(inline_account_id)
    .bind(journal.id())
    .fetch_one(&pool)
    .await?
    .try_get("cnt")?;

    let ec_count: i64 = sqlx::query(
        "SELECT COUNT(*) as cnt FROM cala_balance_history WHERE account_id = $1 AND journal_id = $2",
    )
    .bind(ec_account_id)
    .bind(journal.id())
    .fetch_one(&pool)
    .await?
    .try_get("cnt")?;

    assert_eq!(
        inline_count, ec_count,
        "EC set should have same number of balance_history rows as inline set"
    );

    // Verify each BTC snapshot's running balance matches
    let inline_history = sqlx::query(
        "SELECT values FROM cala_balance_history WHERE account_id = $1 AND journal_id = $2 AND currency = $3 ORDER BY version",
    )
    .bind(inline_account_id)
    .bind(journal.id())
    .bind(btc.code())
    .fetch_all(&pool)
    .await?;

    let ec_history = sqlx::query(
        "SELECT values FROM cala_balance_history WHERE account_id = $1 AND journal_id = $2 AND currency = $3 ORDER BY version",
    )
    .bind(ec_account_id)
    .bind(journal.id())
    .bind(btc.code())
    .fetch_all(&pool)
    .await?;

    assert_eq!(
        inline_history.len(),
        ec_history.len(),
        "BTC history count mismatch"
    );
    for (inline_row, ec_row) in inline_history.iter().zip(ec_history.iter()) {
        let i_snap: BalanceSnapshot =
            serde_json::from_value(inline_row.try_get::<serde_json::Value, _>("values")?)?;
        let e_snap: BalanceSnapshot =
            serde_json::from_value(ec_row.try_get::<serde_json::Value, _>("values")?)?;
        assert_eq!(
            i_snap.version, e_snap.version,
            "version mismatch at v{}",
            i_snap.version
        );
        assert_eq!(
            i_snap.settled.dr_balance, e_snap.settled.dr_balance,
            "settled dr mismatch at v{}",
            i_snap.version
        );
        assert_eq!(
            i_snap.settled.cr_balance, e_snap.settled.cr_balance,
            "settled cr mismatch at v{}",
            i_snap.version
        );
        assert_eq!(
            i_snap.entry_id, e_snap.entry_id,
            "entry_id mismatch at v{}",
            i_snap.version
        );
    }

    // Idempotency: calling recalculate again should be a no-op
    cala.account_sets()
        .recalculate_balances(ec_set.id())
        .await
        .unwrap();
    let ec_bal_3 = cala.balances().find(journal.id(), ec_set.id(), btc).await?;
    assert_eq!(
        ec_bal_2.settled(),
        ec_bal_3.settled(),
        "should be idempotent"
    );
    assert_eq!(
        ec_bal_2.details.version, ec_bal_3.details.version,
        "version should not change on idempotent call"
    );

    Ok(())
}

#[tokio::test]
async fn batch_recalculate_shared_members() -> anyhow::Result<()> {
    let btc: Currency = "BTC".parse().unwrap();

    let pool = helpers::init_pool().await?;
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

    let (sender, receiver) = helpers::test_accounts();
    let sender_account = cala.accounts().create(sender).await.unwrap();
    let recipient_account = cala.accounts().create(receiver).await.unwrap();

    // Create a second leaf account that only belongs to set_b
    let extra = NewAccount::builder()
        .id(uuid::Uuid::now_v7())
        .name("Extra Account")
        .code(Alphanumeric.sample_string(&mut rand::rng(), 32))
        .build()
        .unwrap();
    let extra_account = cala.accounts().create(extra).await.unwrap();

    let tx_code = Alphanumeric.sample_string(&mut rand::rng(), 32);
    cala.tx_templates()
        .create(helpers::currency_conversion_template(&tx_code))
        .await
        .unwrap();

    // Build 3-level hierarchy:
    //   root → [set_a, set_b]
    //   set_a has: recipient_account (exclusive)
    //   set_b has: extra_account (exclusive)
    // Members are exclusive to avoid MemberAlreadyAdded on transitive cascade.
    let set_a = NewAccountSet::builder()
        .id(AccountSetId::new())
        .name("Set A")
        .journal_id(journal.id())
        .eventually_consistent(true)
        .build()
        .unwrap();
    let set_a = cala.account_sets().create(set_a).await.unwrap();

    let set_b = NewAccountSet::builder()
        .id(AccountSetId::new())
        .name("Set B")
        .journal_id(journal.id())
        .eventually_consistent(true)
        .build()
        .unwrap();
    let set_b = cala.account_sets().create(set_b).await.unwrap();

    let root = NewAccountSet::builder()
        .id(AccountSetId::new())
        .name("Root")
        .journal_id(journal.id())
        .eventually_consistent(true)
        .build()
        .unwrap();
    let root = cala.account_sets().create(root).await.unwrap();

    // Wire membership: each child set gets its own exclusive leaf account
    cala.account_sets()
        .add_member(set_a.id(), recipient_account.id())
        .await
        .unwrap();
    cala.account_sets()
        .add_member(set_b.id(), extra_account.id())
        .await
        .unwrap();
    cala.account_sets()
        .add_member(root.id(), set_a.id())
        .await
        .unwrap();
    cala.account_sets()
        .add_member(root.id(), set_b.id())
        .await
        .unwrap();

    // Post transactions: recipient gets BTC credits, sender gets BTC debits
    for _ in 0..3 {
        let mut params = Params::new();
        params.insert("journal_id", journal.id().to_string());
        params.insert("sender", sender_account.id());
        params.insert("recipient", recipient_account.id());
        cala.post_transaction(TransactionId::new(), &tx_code, params)
            .await
            .unwrap();
    }

    // Also post a transaction where extra_account is sender (BTC debit to extra)
    let tx_code_extra = Alphanumeric.sample_string(&mut rand::rng(), 32);
    cala.tx_templates()
        .create(helpers::currency_conversion_template(&tx_code_extra))
        .await
        .unwrap();
    for _ in 0..2 {
        let mut params = Params::new();
        params.insert("journal_id", journal.id().to_string());
        params.insert("sender", extra_account.id());
        params.insert("recipient", sender_account.id());
        cala.post_transaction(TransactionId::new(), &tx_code_extra, params)
            .await
            .unwrap();
    }

    // Batch recalculate all three sets at once
    cala.account_sets()
        .recalculate_balances_batch(&[root.id(), set_a.id(), set_b.id()])
        .await
        .unwrap();

    // Verify set_a: should reflect recipient's balance only
    let recipient_bal = cala
        .balances()
        .find(journal.id(), recipient_account.id(), btc)
        .await?;
    let set_a_bal = cala.balances().find(journal.id(), set_a.id(), btc).await?;
    assert_eq!(
        recipient_bal.settled(),
        set_a_bal.settled(),
        "set_a should match recipient"
    );

    // Verify set_b: should reflect extra's balance only
    let extra_bal = cala
        .balances()
        .find(journal.id(), extra_account.id(), btc)
        .await?;
    let set_b_bal = cala.balances().find(journal.id(), set_b.id(), btc).await?;
    assert_eq!(
        extra_bal.settled(),
        set_b_bal.settled(),
        "set_b should match extra"
    );

    // Verify root: should be recipient + extra (transitive members from both child sets)
    let root_bal = cala.balances().find(journal.id(), root.id(), btc).await?;
    let expected_root = recipient_bal.settled() + extra_bal.settled();
    assert_eq!(
        expected_root,
        root_bal.settled(),
        "root should be recipient + extra"
    );

    // Idempotency: calling batch recalculate again should be a no-op
    cala.account_sets()
        .recalculate_balances_batch(&[root.id(), set_a.id(), set_b.id()])
        .await
        .unwrap();
    let root_bal_2 = cala.balances().find(journal.id(), root.id(), btc).await?;
    assert_eq!(
        root_bal.settled(),
        root_bal_2.settled(),
        "batch recalculate should be idempotent"
    );
    assert_eq!(
        root_bal.details.version, root_bal_2.details.version,
        "version should not change on idempotent batch recalculate"
    );

    Ok(())
}

#[tokio::test]
async fn deep_recalculate_expands_to_descendants() -> anyhow::Result<()> {
    let btc: Currency = "BTC".parse().unwrap();

    let pool = helpers::init_pool().await?;
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

    let (sender, receiver) = helpers::test_accounts();
    let sender_account = cala.accounts().create(sender).await.unwrap();
    let recipient_account = cala.accounts().create(receiver).await.unwrap();

    let tx_code = Alphanumeric.sample_string(&mut rand::rng(), 32);
    cala.tx_templates()
        .create(helpers::currency_conversion_template(&tx_code))
        .await
        .unwrap();

    // Build: root → child, child has recipient as leaf member
    let child = NewAccountSet::builder()
        .id(AccountSetId::new())
        .name("Child")
        .journal_id(journal.id())
        .eventually_consistent(true)
        .build()
        .unwrap();
    let child = cala.account_sets().create(child).await.unwrap();

    let root = NewAccountSet::builder()
        .id(AccountSetId::new())
        .name("Root")
        .journal_id(journal.id())
        .eventually_consistent(true)
        .build()
        .unwrap();
    let root = cala.account_sets().create(root).await.unwrap();

    cala.account_sets()
        .add_member(child.id(), recipient_account.id())
        .await
        .unwrap();
    cala.account_sets()
        .add_member(root.id(), child.id())
        .await
        .unwrap();

    // Post transactions
    for _ in 0..3 {
        let mut params = Params::new();
        params.insert("journal_id", journal.id().to_string());
        params.insert("sender", sender_account.id());
        params.insert("recipient", recipient_account.id());
        cala.post_transaction(TransactionId::new(), &tx_code, params)
            .await
            .unwrap();
    }

    // Deep recalculate from root only — child should also be recalculated
    cala.account_sets()
        .recalculate_balances_deep(&[root.id()])
        .await
        .unwrap();

    let recipient_bal = cala
        .balances()
        .find(journal.id(), recipient_account.id(), btc)
        .await?;
    let child_bal = cala.balances().find(journal.id(), child.id(), btc).await?;
    let root_bal = cala.balances().find(journal.id(), root.id(), btc).await?;

    assert_eq!(
        recipient_bal.settled(),
        child_bal.settled(),
        "child should match recipient"
    );
    assert_eq!(
        recipient_bal.settled(),
        root_bal.settled(),
        "root should match recipient (same transitive member)"
    );

    // Idempotency
    cala.account_sets()
        .recalculate_balances_deep(&[root.id()])
        .await
        .unwrap();
    let root_bal_2 = cala.balances().find(journal.id(), root.id(), btc).await?;
    assert_eq!(root_bal.details.version, root_bal_2.details.version);

    Ok(())
}

#[tokio::test]
async fn list_eventually_consistent_ids() -> anyhow::Result<()> {
    let pool = helpers::init_pool().await?;
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

    let inline_set = NewAccountSet::builder()
        .id(AccountSetId::new())
        .name("Inline Set")
        .journal_id(journal.id())
        .build()
        .unwrap();
    let inline_set = cala.account_sets().create(inline_set).await.unwrap();

    let mut expected_ec_ids = Vec::new();
    for i in 0..3 {
        let ec_set = NewAccountSet::builder()
            .id(AccountSetId::new())
            .name(format!("EC Set {i}"))
            .journal_id(journal.id())
            .eventually_consistent(true)
            .build()
            .unwrap();
        let ec_set = cala.account_sets().create(ec_set).await.unwrap();
        expected_ec_ids.push(ec_set.id());
    }

    // Walk the full list in pages of 2 and collect all returned ids in order.
    let mut collected = Vec::new();
    let mut after: Option<AccountSetByIdCursor> = None;
    loop {
        let ret = cala
            .account_sets()
            .list_eventually_consistent_ids(es_entity::PaginatedQueryArgs {
                first: 2,
                after: after.take(),
            })
            .await?;
        assert!(ret.entities.len() <= 2, "page should respect `first` limit");
        collected.extend(ret.entities);
        if !ret.has_next_page {
            break;
        }
        after = ret.end_cursor;
        assert!(
            after.is_some(),
            "next page requires an end cursor when has_next_page is true"
        );
    }

    // EC ids should come out sorted by id ascending across pages.
    let mut prev: Option<AccountSetId> = None;
    for id in &collected {
        if let Some(p) = prev {
            assert!(p < *id, "ids must be strictly ascending across pages");
        }
        prev = Some(*id);
    }

    for id in &expected_ec_ids {
        assert!(
            collected.contains(id),
            "eventually consistent set {id} should be listed across pages"
        );
    }
    assert!(
        !collected.contains(&inline_set.id()),
        "inline set should not be listed as eventually consistent"
    );

    Ok(())
}

/// `add_member` must reject candidates that already have any
/// `cala_balance_history` rows in the journal: the only way to honour
/// pre-existing balance would be to fold it synchronously, but that fold
/// races with concurrent posters of *other* members for EC sets and has
/// no race-free analogue we want to maintain for non-EC sets either.
#[tokio::test]
async fn add_member_errors_when_member_has_history() -> anyhow::Result<()> {
    let pool = helpers::init_pool().await?;
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

/// `recalculate_balances` (and the batch / deep variants that funnel
/// through `recalculate_balances_batch_in_op`) must reject non-EC
/// account sets up front. Non-EC sets are maintained inline by posters
/// and recalculating them is unsupported (it would race with the
/// within-batch `nextval` ordering on the watermark and risk
/// double-counting member-history rows that the original poster's
/// fold-up already accounted for).
#[tokio::test]
async fn recalculate_balances_errors_on_non_ec_set() -> anyhow::Result<()> {
    let pool = helpers::init_pool().await?;
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

    let inline_set = cala
        .account_sets()
        .create(
            NewAccountSet::builder()
                .id(AccountSetId::new())
                .name("Inline Set")
                .journal_id(journal.id())
                .build()
                .unwrap(),
        )
        .await
        .unwrap();

    let err = cala
        .account_sets()
        .recalculate_balances(inline_set.id())
        .await
        .err()
        .expect("recalculate_balances should fail on a non-EC set");

    match err {
        AccountSetError::CannotRecalculateNonEcSet { account_set_id } => {
            assert_eq!(account_set_id, inline_set.id());
        }
        other => panic!("expected CannotRecalculateNonEcSet, got {other}"),
    }

    // The same rejection applies to `recalculate_balances_batch`.
    let err = cala
        .account_sets()
        .recalculate_balances_batch(&[inline_set.id()])
        .await
        .err()
        .expect("recalculate_balances_batch should fail on a non-EC set");
    assert!(matches!(
        err,
        AccountSetError::CannotRecalculateNonEcSet { .. }
    ));

    Ok(())
}

/// `recalculate_balances_deep` walks the descendant tree of its inputs
/// but should silently skip non-EC descendants — only the EC ones get
/// recalculated. The non-EC descendant's balance is left untouched.
#[tokio::test]
async fn recalculate_balances_deep_skips_non_ec_descendants() -> anyhow::Result<()> {
    let btc: Currency = "BTC".parse().unwrap();

    let pool = helpers::init_pool().await?;
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

    let (sender, leaf_a) = helpers::test_accounts();
    let sender_account = cala.accounts().create(sender).await.unwrap();
    let leaf_a = cala.accounts().create(leaf_a).await.unwrap();

    // Second leaf account, exclusive to the non-EC subtree, so the
    // transitive-member closure under the EC root has distinct entries
    // for the two children (the unique constraint on
    // `cala_account_set_member_accounts` would otherwise reject adding
    // both children to the root with the same shared leaf).
    let code = Alphanumeric.sample_string(&mut rand::rng(), 32);
    let leaf_b = NewAccount::builder()
        .id(uuid::Uuid::now_v7())
        .name(format!("Leaf B {code}"))
        .code(code)
        .build()
        .unwrap();
    let leaf_b = cala.accounts().create(leaf_b).await.unwrap();

    let tx_code = Alphanumeric.sample_string(&mut rand::rng(), 32);
    cala.tx_templates()
        .create(helpers::currency_conversion_template(&tx_code))
        .await
        .unwrap();

    // Hierarchy:
    //   ec_root (EC)
    //     ├── ec_child     (EC,     contains leaf_a)
    //     └── inline_child (non-EC, contains leaf_b)
    let ec_root = cala
        .account_sets()
        .create(
            NewAccountSet::builder()
                .id(AccountSetId::new())
                .name("EC Root")
                .journal_id(journal.id())
                .eventually_consistent(true)
                .build()
                .unwrap(),
        )
        .await
        .unwrap();
    let ec_child = cala
        .account_sets()
        .create(
            NewAccountSet::builder()
                .id(AccountSetId::new())
                .name("EC Child")
                .journal_id(journal.id())
                .eventually_consistent(true)
                .build()
                .unwrap(),
        )
        .await
        .unwrap();
    let inline_child = cala
        .account_sets()
        .create(
            NewAccountSet::builder()
                .id(AccountSetId::new())
                .name("Inline Child")
                .journal_id(journal.id())
                .build()
                .unwrap(),
        )
        .await
        .unwrap();

    cala.account_sets()
        .add_member(ec_child.id(), leaf_a.id())
        .await
        .unwrap();
    cala.account_sets()
        .add_member(inline_child.id(), leaf_b.id())
        .await
        .unwrap();
    cala.account_sets()
        .add_member(ec_root.id(), ec_child.id())
        .await
        .unwrap();
    cala.account_sets()
        .add_member(ec_root.id(), inline_child.id())
        .await
        .unwrap();

    // Post to leaf_a — propagates up through the EC chain (ec_child,
    // ec_root) only as membership; posters do not write
    // `cala_current_balances` rows for EC ancestors, so neither
    // ec_child nor ec_root has a balance until we recalc.
    let mut params = Params::new();
    params.insert("journal_id", journal.id().to_string());
    params.insert("sender", sender_account.id());
    params.insert("recipient", leaf_a.id());
    cala.post_transaction(TransactionId::new(), &tx_code, params)
        .await
        .unwrap();

    // Post to leaf_b — propagates up through the non-EC inline_child
    // synchronously (its `cala_current_balances` row is updated by the
    // poster's fold-up). The EC ec_root ancestor is filtered out of
    // the poster fold and stays at zero.
    let mut params = Params::new();
    params.insert("journal_id", journal.id().to_string());
    params.insert("sender", sender_account.id());
    params.insert("recipient", leaf_b.id());
    cala.post_transaction(TransactionId::new(), &tx_code, params)
        .await
        .unwrap();

    // Snapshot inline_child's balance before the deep recalc, so we
    // can prove the deep recalc leaves it untouched.
    let inline_before = cala
        .balances()
        .find(journal.id(), inline_child.id(), btc)
        .await?;

    cala.account_sets()
        .recalculate_balances_deep(&[ec_root.id()])
        .await
        .unwrap();

    let leaf_a_bal = cala.balances().find(journal.id(), leaf_a.id(), btc).await?;
    let leaf_b_bal = cala.balances().find(journal.id(), leaf_b.id(), btc).await?;
    let ec_child_bal = cala
        .balances()
        .find(journal.id(), ec_child.id(), btc)
        .await?;
    let ec_root_bal = cala
        .balances()
        .find(journal.id(), ec_root.id(), btc)
        .await?;
    let inline_after = cala
        .balances()
        .find(journal.id(), inline_child.id(), btc)
        .await?;

    // The EC descendant got recalculated and now reflects its own
    // exclusive leaf member.
    assert_eq!(
        leaf_a_bal.settled(),
        ec_child_bal.settled(),
        "ec_child should match leaf_a after deep recalc"
    );

    // The EC root sees both transitive leaves (leaf_a via ec_child and
    // leaf_b via inline_child), regardless of whether the path runs
    // through an EC or non-EC intermediate set.
    assert_eq!(
        leaf_a_bal.settled() + leaf_b_bal.settled(),
        ec_root_bal.settled(),
        "ec_root should equal leaf_a + leaf_b after deep recalc"
    );

    // The non-EC descendant must be left exactly as it was before the
    // deep recalc — no version bump, no balance change.
    assert_eq!(
        inline_before.settled(),
        inline_after.settled(),
        "non-EC descendant settled balance should be untouched by deep recalc"
    );
    assert_eq!(
        inline_before.details.version, inline_after.details.version,
        "non-EC descendant version should not change"
    );

    Ok(())
}

#[tokio::test]
async fn ec_account_set_entry_listing() -> anyhow::Result<()> {
    let pool = helpers::init_pool().await?;
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

    let (sender, receiver) = helpers::test_accounts();
    let sender_account = cala.accounts().create(sender).await.unwrap();
    let recipient_account = cala.accounts().create(receiver).await.unwrap();

    let tx_code = Alphanumeric.sample_string(&mut rand::rng(), 32);
    cala.tx_templates()
        .create(helpers::currency_conversion_template(&tx_code))
        .await
        .unwrap();

    // Create an eventually_consistent account set
    let ec_set = NewAccountSet::builder()
        .id(AccountSetId::new())
        .name("EC Entry Listing Set")
        .journal_id(journal.id())
        .eventually_consistent(true)
        .build()
        .unwrap();
    let ec_set = cala.account_sets().create(ec_set).await.unwrap();

    // Add recipient as a member
    cala.account_sets()
        .add_member(ec_set.id(), recipient_account.id())
        .await
        .unwrap();

    // Post first transaction
    let mut params1 = Params::new();
    params1.insert("journal_id", journal.id().to_string());
    params1.insert("sender", sender_account.id());
    params1.insert("recipient", recipient_account.id());
    cala.post_transaction(TransactionId::new(), &tx_code, params1)
        .await
        .unwrap();

    // Query entries for the EC set — should be non-empty
    let query_args = es_entity::PaginatedQueryArgs {
        first: 10,
        after: None,
    };
    let ret = cala
        .entries()
        .list_for_account_set_id(ec_set.id(), query_args, es_entity::ListDirection::Ascending)
        .await?;
    assert!(
        !ret.entities.is_empty(),
        "EC account set entry listing should return entries"
    );
    let first_tx_entry_count = ret.entities.len();

    // Post second transaction
    let mut params2 = Params::new();
    params2.insert("journal_id", journal.id().to_string());
    params2.insert("sender", sender_account.id());
    params2.insert("recipient", recipient_account.id());
    cala.post_transaction(TransactionId::new(), &tx_code, params2)
        .await
        .unwrap();

    // Paginated query: first page with limit 1
    let page1_args = es_entity::PaginatedQueryArgs {
        first: first_tx_entry_count,
        after: None,
    };
    let page1 = cala
        .entries()
        .list_for_account_set_id(ec_set.id(), page1_args, es_entity::ListDirection::Ascending)
        .await?;
    assert!(page1.has_next_page, "should have a second page");
    assert!(page1.end_cursor.is_some(), "should have an end cursor");

    // Second page using cursor
    let page2_args = es_entity::PaginatedQueryArgs {
        first: 10,
        after: page1.end_cursor,
    };
    let page2 = cala
        .entries()
        .list_for_account_set_id(ec_set.id(), page2_args, es_entity::ListDirection::Ascending)
        .await?;
    assert!(
        !page2.entities.is_empty(),
        "second page should have entries"
    );

    // Descending order should return entries in reverse
    let desc_args = es_entity::PaginatedQueryArgs {
        first: 20,
        after: None,
    };
    let desc_ret = cala
        .entries()
        .list_for_account_set_id(ec_set.id(), desc_args, es_entity::ListDirection::Descending)
        .await?;
    assert!(
        !desc_ret.entities.is_empty(),
        "descending listing should return entries"
    );

    // Verify descending order: first entry should have created_at >= last entry
    if desc_ret.entities.len() > 1 {
        let first = &desc_ret.entities[0];
        let last = &desc_ret.entities[desc_ret.entities.len() - 1];
        assert!(
            first.created_at() >= last.created_at(),
            "descending order should have newest first"
        );
    }

    Ok(())
}
