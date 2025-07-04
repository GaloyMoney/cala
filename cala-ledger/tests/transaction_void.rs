mod helpers;

use rand::distr::{Alphanumeric, SampleString};

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

    let original_tx_id = TransactionId::new();

    let tx = cala
        .post_transaction(original_tx_id, &tx_code, params)
        .await
        .unwrap();

    let voided_tx = cala.void_transaction(original_tx_id).await.unwrap();

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

        let mut original_units = original_entry.values().units;
        original_units.set_sign_negative(true);
        assert_eq!(original_units, voided_entry.values().units);
    }

    Ok(())
}
