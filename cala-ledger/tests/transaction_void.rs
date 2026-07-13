mod helpers;

use chrono::NaiveDate;
use rand::distr::{Alphanumeric, SampleString};
use rust_decimal_macros::dec;

use cala_ledger::{error::LedgerError, transaction::error::TransactionError, tx_template::*, *};

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

    let existing_tx_id = TransactionId::new();

    let tx = cala
        .post_transaction(existing_tx_id, &tx_code, params)
        .await
        .unwrap();

    let voiding_tx_id = TransactionId::new();
    let voided_tx = cala
        .void_transaction(
            voiding_tx_id,
            existing_tx_id,
            chrono::Utc::now().date_naive(),
        )
        .await
        .unwrap();

    let original_tx_entries = cala.entries().find_all(&tx.values().entry_ids).await?;
    let mut original_entries: Vec<_> = original_tx_entries.values().collect();
    original_entries.sort_by_key(|entry| entry.values().sequence);

    let voided_tx_entries = cala
        .entries()
        .find_all(&voided_tx.values().entry_ids)
        .await?;
    let mut voided_entries: Vec<_> = voided_tx_entries.values().collect();
    voided_entries.sort_by_key(|entry| entry.values().sequence);

    for (original_entry, voided_entry) in original_entries.iter().zip(voided_entries.iter()) {
        assert!(voided_entry.values().entry_type.ends_with("_VOID"));

        assert_eq!(-original_entry.values().units, voided_entry.values().units);
    }

    Ok(())
}

#[tokio::test]
async fn transaction_void_with_effective() -> anyhow::Result<()> {
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

    let effective = NaiveDate::from_ymd_opt(2025, 5, 5).unwrap();

    let mut params = Params::new();
    params.insert("journal_id", journal.id().to_string());
    params.insert("sender", sender_account.id());
    params.insert("recipient", recipient_account.id());
    params.insert("effective", effective);

    let existing_tx_id = TransactionId::new();
    cala.post_transaction(existing_tx_id, &tx_code, params)
        .await
        .unwrap();

    let res = cala
        .void_transaction(
            TransactionId::new(),
            existing_tx_id,
            effective - chrono::Days::new(1),
        )
        .await;
    assert!(matches!(
        res,
        Err(LedgerError::TransactionError(
            TransactionError::VoidEffectiveBeforeOriginal { .. }
        ))
    ));

    let voiding_tx_id = TransactionId::new();
    let voided_tx = cala
        .void_transaction(voiding_tx_id, existing_tx_id, effective)
        .await
        .unwrap();
    assert_eq!(voided_tx.effective(), effective);

    let recipient_balance = cala
        .balances()
        .effective()
        .find_cumulative(
            journal.id(),
            recipient_account.id(),
            Currency::BTC,
            effective,
        )
        .await?;
    assert_eq!(recipient_balance.settled(), dec!(0));

    let recipient_balance = cala
        .balances()
        .effective()
        .find_cumulative(
            journal.id(),
            recipient_account.id(),
            Currency::USD,
            effective,
        )
        .await?;
    assert_eq!(recipient_balance.settled(), dec!(0));
    assert_eq!(recipient_balance.pending(), dec!(0));

    Ok(())
}
