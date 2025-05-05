mod helpers;

use chrono::NaiveDate;
use rand::distr::{Alphanumeric, SampleString};

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

    let _tx = cala
        .post_transaction(TransactionId::new(), &tx_code, params)
        .await
        .unwrap();

    // Run it again to test balance updates
    let mut params = Params::new();
    params.insert("journal_id", journal.id());
    params.insert("sender", sender_account.id());
    params.insert("recipient", recipient_account.id());
    let date2 = NaiveDate::from_ymd_opt(2025, 5, 4).unwrap();
    params.insert("effective", date2);

    cala.post_transaction(TransactionId::new(), &tx_code, params)
        .await
        .unwrap();
    // let recipient_balance = cala
    //     .balances()
    //     .effective()
    //     .find_cumulative(
    //         journal.id(),
    //         recipient_account.id(),
    //         "BTC".parse().unwrap(),
    //         date2,
    //     )
    //     .await?;

    // assert!(true);
    Ok(())
}
