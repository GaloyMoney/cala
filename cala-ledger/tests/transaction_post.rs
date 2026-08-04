mod helpers;

use std::collections::HashMap;

use rand::distr::{Alphanumeric, SampleString};
use rust_decimal::Decimal;

use cala_ledger::{account_set::*, error::LedgerError, primitives::BalanceRollup, tx_template::*, *};

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
async fn errors_when_entry_targets_eventually_consistent_account() -> anyhow::Result<()> {
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
    let (sender, recipient) = helpers::test_accounts();
    let sender_account = cala.accounts().create(sender).await.unwrap();
    let recipient_account = cala.accounts().create(recipient).await.unwrap();

    let ec_set = NewAccountSet::builder()
        .id(AccountSetId::new())
        .name("EC SET")
        .journal_id(journal.id())
        .balance_rollup(BalanceRollup::EventuallyConsistent)
        .build()
        .unwrap();
    let ec_set = cala.account_sets().create(ec_set).await.unwrap();

    let tx_code = Alphanumeric.sample_string(&mut rand::rng(), 32);
    let params_def = vec![
        NewParamDefinition::builder()
            .name("recipient")
            .r#type(ParamDataType::Uuid)
            .build()
            .unwrap(),
        NewParamDefinition::builder()
            .name("sender")
            .r#type(ParamDataType::Uuid)
            .build()
            .unwrap(),
        NewParamDefinition::builder()
            .name("journal_id")
            .r#type(ParamDataType::Uuid)
            .build()
            .unwrap(),
        NewParamDefinition::builder()
            .name("effective")
            .r#type(ParamDataType::Date)
            .default_expr("date()")
            .build()
            .unwrap(),
    ];
    let entries = vec![
        NewTxTemplateEntry::builder()
            .entry_type("'DR'")
            .account_id("params.sender")
            .layer("SETTLED")
            .direction("DEBIT")
            .units("decimal('100')")
            .currency("'USD'")
            .build()
            .unwrap(),
        NewTxTemplateEntry::builder()
            .entry_type("'CR'")
            .account_id("params.recipient")
            .layer("SETTLED")
            .direction("CREDIT")
            .units("decimal('100')")
            .currency("'USD'")
            .build()
            .unwrap(),
    ];
    let new_template = NewTxTemplate::builder()
        .id(uuid::Uuid::now_v7())
        .code(&tx_code)
        .params(params_def)
        .transaction(
            NewTxTemplateTransaction::builder()
                .effective("params.effective")
                .journal_id("params.journal_id")
                .build()
                .unwrap(),
        )
        .entries(entries)
        .build()
        .unwrap();
    cala.tx_templates().create(new_template).await.unwrap();

    let make_params = |recipient: AccountId| {
        let mut params = Params::new();
        params.insert("journal_id", journal.id());
        params.insert("sender", sender_account.id());
        params.insert("recipient", recipient);
        params
    };

    // Posting directly to an EC account set's underlying account would
    // persist the entry without ever reflecting it in any balance
    let res = cala
        .post_transaction(
            TransactionId::new(),
            &tx_code,
            make_params(AccountId::from(ec_set.id())),
        )
        .await;
    assert!(matches!(
        res,
        Err(LedgerError::EntriesTargetEventuallyConsistentAccount(_))
    ));

    // Posting to regular accounts works
    let res = cala
        .post_transaction(
            TransactionId::new(),
            &tx_code,
            make_params(recipient_account.id()),
        )
        .await;
    assert!(res.is_ok());

    Ok(())
}
