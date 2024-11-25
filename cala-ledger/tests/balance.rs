mod helpers;

use chrono::{TimeZone, Utc};
use rust_decimal::Decimal;

use cala_ledger::{tx_template::*, *};
use rand::distributions::{Alphanumeric, DistString};

#[tokio::test]
async fn balance_in_range() -> anyhow::Result<()> {
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
    let new_template = helpers::currency_conversion_template(&tx_code);

    cala.tx_templates().create(new_template).await.unwrap();
    let mut params = Params::new();
    params.insert("journal_id", journal.id().to_string());
    params.insert("sender", sender_account.id());
    params.insert("recipient", recipient_account.id());

    cala.post_transaction(TransactionId::new(), &tx_code, params)
        .await
        .unwrap();

    let range = cala
        .balances()
        .find_in_range(
            journal.id(),
            recipient_account.id(),
            btc,
            Utc.timestamp_opt(0, 0).single().unwrap(),
            None,
        )
        .await?;

    assert_eq!(range.start.settled(), Decimal::ZERO);
    assert_eq!(range.end.settled(), Decimal::from(1290));
    assert_eq!(range.diff.settled(), Decimal::from(1290));
    assert_eq!(range.end.details.version, 1);

    let after_first_before_second_tx = Utc::now();
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    let mut params = Params::new();
    params.insert("journal_id", journal.id().to_string());
    params.insert("sender", sender_account.id());
    params.insert("recipient", recipient_account.id());

    cala.post_transaction(TransactionId::new(), &tx_code, params)
        .await
        .unwrap();

    let range = cala
        .balances()
        .find_in_range(
            journal.id(),
            recipient_account.id(),
            btc,
            Utc.timestamp_opt(0, 0).single().unwrap(),
            Some(after_first_before_second_tx),
        )
        .await?;

    assert_eq!(range.start.settled(), Decimal::ZERO);
    assert_eq!(range.end.settled(), Decimal::from(1290));
    assert_eq!(range.diff.settled(), Decimal::from(1290));
    assert_eq!(range.end.details.version, 1);

    let range = cala
        .balances()
        .find_in_range(
            journal.id(),
            recipient_account.id(),
            btc,
            after_first_before_second_tx,
            None,
        )
        .await?;

    assert_eq!(range.start.settled(), Decimal::from(1290));
    assert_eq!(range.end.settled(), Decimal::from(2580));
    assert_eq!(range.diff.settled(), Decimal::from(1290));
    assert_eq!(range.end.details.version, 2);

    Ok(())
}
