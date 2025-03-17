mod helpers;

use rand::distr::{Alphanumeric, SampleString};
use rust_decimal::Decimal;

use cala_ledger::{tx_template::*, *};

#[tokio::test]
async fn transaction_post() -> anyhow::Result<()> {
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

    let mut params = Params::new();
    params.insert("journal_id", journal.id().to_string());
    params.insert("sender", sender_account.id());
    params.insert("recipient", recipient_account.id());

    cala.post_transaction(TransactionId::new(), &tx_code, params)
        .await
        .unwrap();

    // Run it again to test balance updates
    let mut params = Params::new();
    params.insert("journal_id", journal.id());
    params.insert("sender", sender_account.id());
    params.insert("recipient", recipient_account.id());
    cala.post_transaction(TransactionId::new(), &tx_code, params)
        .await
        .unwrap();
    let recipient_balance = cala
        .balances()
        .find(journal.id(), recipient_account.id(), "BTC".parse().unwrap())
        .await?;
    assert_eq!(recipient_balance.settled(), Decimal::from(1290 * 2));
    let all_balances = cala
        .balances()
        .find_all(&[
            (journal.id(), recipient_account.id(), "BTC".parse().unwrap()),
            (journal.id(), sender_account.id(), "BTC".parse().unwrap()),
        ])
        .await?;
    let sender_balance = all_balances
        .get(&(journal.id(), sender_account.id(), "BTC".parse().unwrap()))
        .unwrap();
    assert_eq!(sender_balance.settled(), Decimal::from(-1290 * 2));

    Ok(())
}
