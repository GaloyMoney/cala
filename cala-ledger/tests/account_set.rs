mod helpers;

use rand::distr::{Alphanumeric, SampleString};

use cala_ledger::{account::*, account_set::*, tx_template::*, *};

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

    let before_account_set = NewAccountSet::builder()
        .id(AccountSetId::new())
        .name("Before")
        .journal_id(journal.id())
        .build()
        .unwrap();
    let before_set = cala
        .account_sets()
        .create(before_account_set)
        .await
        .unwrap();
    let parent_set = NewAccountSet::builder()
        .id(AccountSetId::new())
        .name("Parent")
        .journal_id(journal.id())
        .build()
        .unwrap();
    let parent_set = cala.account_sets().create(parent_set).await.unwrap();

    cala.account_sets()
        .add_member(before_set.id(), recipient_account.id())
        .await
        .unwrap();
    cala.account_sets()
        .add_member(parent_set.id(), before_set.id())
        .await
        .unwrap();

    let mut params = Params::new();
    params.insert("journal_id", journal.id().to_string());
    params.insert("sender", sender_account.id());
    params.insert("recipient", recipient_account.id());
    cala.post_transaction(TransactionId::new(), &tx_code, params)
        .await
        .unwrap();

    let recipient_balance = cala
        .balances()
        .find(journal.id(), recipient_account.id(), btc)
        .await?;
    let set_balance = cala
        .balances()
        .find(journal.id(), before_set.id(), btc)
        .await?;
    let parent_balance = cala
        .balances()
        .find(journal.id(), parent_set.id(), btc)
        .await?;
    assert_eq!(recipient_balance.settled(), set_balance.settled());
    assert_eq!(
        recipient_balance.details.version,
        set_balance.details.version
    );
    assert_eq!(
        recipient_balance.details.entry_id,
        set_balance.details.entry_id
    );
    assert_eq!(recipient_balance.settled(), parent_balance.settled());
    assert_eq!(
        recipient_balance.details.version,
        parent_balance.details.version
    );
    assert_eq!(
        recipient_balance.details.entry_id,
        parent_balance.details.entry_id
    );

    let after_account_set = NewAccountSet::builder()
        .id(AccountSetId::new())
        .name("After")
        .journal_id(journal.id())
        .build()
        .unwrap();
    let after_set = cala.account_sets().create(after_account_set).await.unwrap();

    cala.account_sets()
        .add_member(after_set.id(), sender_account.id())
        .await
        .unwrap();
    let set_balance = cala
        .balances()
        .find(journal.id(), after_set.id(), btc)
        .await?;
    let sender_balance = cala
        .balances()
        .find(journal.id(), sender_account.id(), btc)
        .await?;
    assert_eq!(sender_balance.settled(), set_balance.settled());
    assert_eq!(sender_balance.details.version, set_balance.details.version);

    cala.account_sets()
        .add_member(parent_set.id(), after_set.id())
        .await
        .unwrap();
    let parent_balance = cala
        .balances()
        .find(journal.id(), parent_set.id(), btc)
        .await?;

    assert_eq!(parent_balance.settled(), rust_decimal::Decimal::ZERO);
    assert_eq!(parent_balance.details.version, 2);

    cala.account_sets()
        .remove_member(parent_set.id(), before_set.id())
        .await
        .unwrap();
    let parent_balance = cala
        .balances()
        .find(journal.id(), parent_set.id(), btc)
        .await?;
    assert_eq!(parent_balance.settled(), sender_balance.settled());

    cala.account_sets()
        .remove_member(after_set.id(), sender_account.id())
        .await
        .unwrap();
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
            before_set.id(),
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
async fn eventually_consistent_member_add_with_existing_balance() -> anyhow::Result<()> {
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

    // Post transactions so recipient has pre-existing balance
    for _ in 0..2 {
        let mut params = Params::new();
        params.insert("journal_id", journal.id().to_string());
        params.insert("sender", sender_account.id());
        params.insert("recipient", recipient_account.id());
        cala.post_transaction(TransactionId::new(), &tx_code, params)
            .await
            .unwrap();
    }

    // Create inline set for reference
    let inline_set = NewAccountSet::builder()
        .id(AccountSetId::new())
        .name("Inline Set")
        .journal_id(journal.id())
        .build()
        .unwrap();
    let inline_set = cala.account_sets().create(inline_set).await.unwrap();

    // Create EC set
    let ec_set = NewAccountSet::builder()
        .id(AccountSetId::new())
        .name("EC Set")
        .journal_id(journal.id())
        .eventually_consistent(true)
        .build()
        .unwrap();
    let ec_set = cala.account_sets().create(ec_set).await.unwrap();

    // Add recipient (with pre-existing balance) to both sets
    cala.account_sets()
        .add_member(inline_set.id(), recipient_account.id())
        .await
        .unwrap();
    cala.account_sets()
        .add_member(ec_set.id(), recipient_account.id())
        .await
        .unwrap();

    // EC set should now have a balance from add_member summary entries
    let inline_bal = cala
        .balances()
        .find(journal.id(), inline_set.id(), btc)
        .await?;
    let ec_bal = cala.balances().find(journal.id(), ec_set.id(), btc).await?;
    assert_eq!(
        inline_bal.settled(),
        ec_bal.settled(),
        "EC set balance should match inline after add_member"
    );

    // Post more transactions
    for _ in 0..2 {
        let mut params = Params::new();
        params.insert("journal_id", journal.id().to_string());
        params.insert("sender", sender_account.id());
        params.insert("recipient", recipient_account.id());
        cala.post_transaction(TransactionId::new(), &tx_code, params)
            .await
            .unwrap();
    }

    // Recalculate EC — incremental should correctly skip pre-existing history
    cala.account_sets()
        .recalculate_balances(ec_set.id())
        .await
        .unwrap();

    // Verify EC matches inline after recalculate
    let inline_bal_2 = cala
        .balances()
        .find(journal.id(), inline_set.id(), btc)
        .await?;
    let ec_bal_2 = cala.balances().find(journal.id(), ec_set.id(), btc).await?;
    assert_eq!(
        inline_bal_2.settled(),
        ec_bal_2.settled(),
        "EC set should match inline after recalculate"
    );
    assert_eq!(
        inline_bal_2.details.version, ec_bal_2.details.version,
        "version should match after recalculate"
    );

    // Idempotency: recalculate again is a no-op
    cala.account_sets()
        .recalculate_balances(ec_set.id())
        .await
        .unwrap();
    let ec_bal_3 = cala.balances().find(journal.id(), ec_set.id(), btc).await?;
    assert_eq!(
        ec_bal_2.settled(),
        ec_bal_3.settled(),
        "recalculate should be idempotent"
    );
    assert_eq!(
        ec_bal_2.details.version, ec_bal_3.details.version,
        "version should not change on idempotent recalculate"
    );

    Ok(())
}
