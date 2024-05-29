mod helpers;

use cala_ledger::{account_set::*, tx_template::*, *};
use rand::distributions::{Alphanumeric, DistString};

#[tokio::test]
async fn post_transaction() -> anyhow::Result<()> {
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

    let name = Alphanumeric.sample_string(&mut rand::thread_rng(), 32);
    let before_account_set = NewAccountSet::builder()
        .id(AccountSetId::new())
        .name(name)
        .journal_id(journal.id())
        .build()
        .unwrap();
    let before_set = cala
        .account_sets()
        .create(before_account_set)
        .await
        .unwrap();

    cala.account_sets()
        .add_member(before_set.id(), recipient_account.id())
        .await
        .unwrap();

    let mut params = TxParams::new();
    params.insert("journal_id", journal.id().to_string());
    params.insert("sender", sender_account.id());
    params.insert("recipient", recipient_account.id());
    cala.post_transaction(TransactionId::new(), &tx_code, Some(params))
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
    assert_eq!(recipient_balance.settled(), set_balance.settled());
    assert_eq!(
        recipient_balance.details.version,
        set_balance.details.version
    );
    assert_eq!(
        recipient_balance.details.entry_id,
        set_balance.details.entry_id
    );

    Ok(())
}
