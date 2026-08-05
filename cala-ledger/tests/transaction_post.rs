mod helpers;

use std::collections::HashMap;

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

    let tx = cala
        .post_transaction(TransactionId::new(), &tx_code, params)
        .await
        .unwrap();

    let entries = cala
        .entries()
        .find_all(&tx.values().entry_ids)
        .await
        .unwrap();

    // Only one of the entries should have metadata.
    for entry in entries.values() {
        if let Some(metadata) = &entry.values().metadata {
            let metadata: HashMap<String, AccountId> =
                serde_json::from_value(metadata.clone()).unwrap();
            assert_eq!(metadata.get("sender"), Some(&sender_account.id()));
            break;
        }
    }

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

#[tokio::test]
async fn transaction_post_with_bounded_concurrency() -> anyhow::Result<()> {
    let pool = helpers::init_pool().await?;
    let cala_config = CalaLedgerConfig::builder()
        .pool(pool)
        .exec_migrations(false)
        .max_concurrent_postings(1usize)
        .build()?;
    let cala = CalaLedger::init(cala_config).await?;

    let journal = cala.journals().create(helpers::test_journal()).await?;
    let (sender, receiver) = helpers::test_accounts();
    let sender_account = cala.accounts().create(sender).await?;
    let recipient_account = cala.accounts().create(receiver).await?;

    let tx_code = Alphanumeric.sample_string(&mut rand::rng(), 32);
    cala.tx_templates()
        .create(helpers::currency_conversion_template(&tx_code))
        .await?;

    let make_params = |journal_id: JournalId| {
        let mut params = Params::new();
        params.insert("journal_id", journal_id);
        params.insert("sender", sender_account.id());
        params.insert("recipient", recipient_account.id());
        params
    };

    // A single permit must cover the full post_transaction path (the
    // outer call and the inner in_op call must not each take a permit).
    cala.post_transaction(TransactionId::new(), &tx_code, make_params(journal.id()))
        .await?;

    // Same for a caller-composed op posting twice sequentially.
    let mut op = cala.begin_operation().await?;
    cala.post_transaction_in_op(
        &mut op,
        TransactionId::new(),
        &tx_code,
        make_params(journal.id()),
    )
    .await?;
    cala.post_transaction_in_op(
        &mut op,
        TransactionId::new(),
        &tx_code,
        make_params(journal.id()),
    )
    .await?;
    op.commit().await?;

    // Concurrent posters complete even with the bound in place.
    let mut handles = Vec::new();
    for _ in 0..4 {
        let cala = cala.clone();
        let tx_code = tx_code.clone();
        let mut params = make_params(journal.id());
        handles.push(tokio::spawn(async move {
            cala.post_transaction(TransactionId::new(), &tx_code, std::mem::take(&mut params))
                .await
        }));
    }
    for handle in handles {
        handle.await??;
    }

    let recipient_balance = cala
        .balances()
        .find(journal.id(), recipient_account.id(), "BTC".parse().unwrap())
        .await?;
    assert_eq!(recipient_balance.settled(), Decimal::from(1290 * 7));

    Ok(())
}
