mod helpers;

use cala_ledger::{account_set::*, tx_template::*, *};
use rand::distributions::{Alphanumeric, DistString};

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

    let tx_code = Alphanumeric.sample_string(&mut rand::thread_rng(), 32);
    let new_template = helpers::test_template(&tx_code);
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

    let mut params = TxParams::new();
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
    let initial_name = Alphanumeric.sample_string(&mut rand::thread_rng(), 32);
    let new_account_set = NewAccountSet::builder()
        .id(AccountSetId::new())
        .name(initial_name.clone())
        .journal_id(journal.id())
        .build()?;

    let mut account_set = cala.account_sets().create(new_account_set).await?;
    assert_eq!(initial_name, account_set.values().name);

    // update account set name and description
    let updated_name = Alphanumeric.sample_string(&mut rand::thread_rng(), 32);
    let mut builder = AccountSetUpdate::default();
    builder.name(updated_name.clone()).build()?;
    account_set.update(builder);
    cala.account_sets().persist(&mut account_set).await?;

    assert_eq!(updated_name, account_set.values().name);
    Ok(())
}
