mod helpers;

use chrono::NaiveDate;
use rand::distr::{Alphanumeric, SampleString};
use rust_decimal_macros::dec;

use cala_ledger::{tx_template::*, *};

#[tokio::test]
async fn transaction_post_with_effective_balances() -> anyhow::Result<()> {
    let pool = helpers::init_pool().await?;
    let cala_config = CalaLedgerConfig::builder()
        .pool(pool)
        .exec_migrations(false)
        .build()?;
    let cala = CalaLedger::init(cala_config).await?;

    let new_journal = helpers::test_journal_with_effective_balances();
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
    let date1 = NaiveDate::from_ymd_opt(2025, 5, 5).unwrap();
    params.insert("effective", date1);

    cala.post_transaction(TransactionId::new(), &tx_code, params)
        .await
        .unwrap();

    let recipient_balance = cala
        .balances()
        .effective()
        .find_cumulative(journal.id(), recipient_account.id(), Currency::BTC, date1)
        .await?;
    assert_eq!(recipient_balance.settled(), dec!(1290));

    let recipient_balance = cala
        .balances()
        .effective()
        .find_cumulative(journal.id(), recipient_account.id(), Currency::USD, date1)
        .await?;
    assert_eq!(recipient_balance.settled(), dec!(100));
    assert_eq!(recipient_balance.pending(), dec!(100));

    let mut params = Params::new();
    params.insert("journal_id", journal.id());
    params.insert("sender", sender_account.id());
    params.insert("recipient", recipient_account.id());
    let date2 = NaiveDate::from_ymd_opt(2025, 5, 4).unwrap();
    params.insert("effective", date2);

    cala.post_transaction(TransactionId::new(), &tx_code, params)
        .await
        .unwrap();

    let recipient_balance = cala
        .balances()
        .effective()
        .find_cumulative(journal.id(), recipient_account.id(), Currency::BTC, date2)
        .await?;
    assert_eq!(recipient_balance.settled(), dec!(1290));
    let recipient_balance = cala
        .balances()
        .effective()
        .find_cumulative(journal.id(), recipient_account.id(), Currency::USD, date2)
        .await?;
    assert_eq!(recipient_balance.settled(), dec!(100));
    assert_eq!(recipient_balance.pending(), dec!(100));

    let recipient_balance = cala
        .balances()
        .effective()
        .find_cumulative(journal.id(), recipient_account.id(), Currency::BTC, date1)
        .await?;
    assert_eq!(recipient_balance.settled(), dec!(2580));

    let recipient_balance = cala
        .balances()
        .effective()
        .find_cumulative(journal.id(), recipient_account.id(), Currency::USD, date1)
        .await?;
    assert_eq!(recipient_balance.settled(), dec!(200));
    assert_eq!(recipient_balance.pending(), dec!(200));

    Ok(())
}
