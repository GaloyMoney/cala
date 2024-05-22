mod helpers;

use rust_decimal::Decimal;

use cala_ledger::{account::*, journal::*, tx_template::*, *};
use rand::distributions::{Alphanumeric, DistString};

#[tokio::test]
async fn post_transaction() -> Result<(), Box<dyn std::error::Error>> {
    let pool = helpers::init_pool().await?;

    let tx_code = Alphanumeric.sample_string(&mut rand::thread_rng(), 32);

    let name = Alphanumeric.sample_string(&mut rand::thread_rng(), 32);
    let new_journal = NewJournal::builder()
        .id(JournalId::new())
        .name(name)
        .build()
        .unwrap();
    let cala_config = CalaLedgerConfig::builder()
        .pool(pool)
        .exec_migrations(false)
        .build()?;
    let cala = CalaLedger::init(cala_config).await.unwrap();

    let journal = cala.journals().create(new_journal).await.unwrap();
    let code = Alphanumeric.sample_string(&mut rand::thread_rng(), 32);
    let new_account = NewAccount::builder()
        .id(uuid::Uuid::new_v4())
        .name(format!("Test Sender Account {code}"))
        .code(code)
        .build()
        .unwrap();
    let sender_account = cala.accounts().create(new_account).await.unwrap();
    let code = Alphanumeric.sample_string(&mut rand::thread_rng(), 32);
    let new_account = NewAccount::builder()
        .id(uuid::Uuid::new_v4())
        .name(format!("Test Recipient Account {code}"))
        .code(code)
        .build()
        .unwrap();
    let recipient_account = cala.accounts().create(new_account).await.unwrap();

    let params = vec![
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
            .name("external_id")
            .r#type(ParamDataType::String)
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
        NewEntryInput::builder()
            .entry_type("'TEST_BTC_DR'")
            .account_id("params.sender")
            .layer("SETTLED")
            .direction("DEBIT")
            .units("decimal('1290')")
            .currency("'BTC'")
            .build()
            .unwrap(),
        NewEntryInput::builder()
            .entry_type("'TEST_BTC_CR'")
            .account_id("params.recipient")
            .layer("SETTLED")
            .direction("CREDIT")
            .units("decimal('1290')")
            .currency("'BTC'")
            .build()
            .unwrap(),
        NewEntryInput::builder()
            .entry_type("'TEST_USD_DR'")
            .account_id("params.sender")
            .layer("SETTLED")
            .direction("DEBIT")
            .units("decimal('100')")
            .currency("'USD'")
            .build()
            .unwrap(),
        NewEntryInput::builder()
            .entry_type("'TEST_USD_CR'")
            .account_id("params.recipient")
            .layer("SETTLED")
            .direction("CREDIT")
            .units("decimal('100')")
            .currency("'USD'")
            .build()
            .unwrap(),
    ];
    let new_template = NewTxTemplate::builder()
        .id(uuid::Uuid::new_v4())
        .code(&tx_code)
        .params(params)
        .tx_input(
            NewTxInput::builder()
                .effective("params.effective")
                .journal_id("params.journal_id")
                .external_id("params.external_id")
                .metadata(r#"{"foo": "bar"}"#)
                .build()
                .unwrap(),
        )
        .entries(entries)
        .build()
        .unwrap();
    cala.tx_templates().create(new_template).await.unwrap();

    let external_id = uuid::Uuid::new_v4().to_string();
    let mut params = TxParams::new();
    params.insert("journal_id", journal.id().to_string());
    params.insert("sender", sender_account.id());
    params.insert("recipient", recipient_account.id());
    params.insert("external_id", external_id.clone());

    cala.post_transaction(TransactionId::new(), &tx_code, Some(params))
        .await
        .unwrap();

    // Run it again to test balance updates
    let external_id = uuid::Uuid::new_v4().to_string();
    let mut params = TxParams::new();
    params.insert("journal_id", journal.id());
    params.insert("sender", sender_account.id());
    params.insert("recipient", recipient_account.id());
    params.insert("external_id", external_id.clone());
    cala.post_transaction(TransactionId::new(), &tx_code, Some(params))
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
    assert_eq!(sender_balance.settled(), Decimal::from(-1 * 1290 * 2));

    Ok(())
}
