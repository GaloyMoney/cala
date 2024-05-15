mod helpers;

// use rust_decimal::Decimal;

use cala_ledger::{account::*, journal::*, tx_template::*, *};
use rand::distributions::{Alphanumeric, DistString};

#[tokio::test]
async fn post_transaction() -> anyhow::Result<()> {
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
    let cala = CalaLedger::init(cala_config).await?;

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
    params.insert("journal_id", journal.id());
    params.insert("sender", sender_account.id());
    params.insert("recipient", recipient_account.id());
    params.insert("external_id", external_id.clone());

    cala.post_transaction(TransactionId::new(), &tx_code, Some(params))
        .await
        .unwrap();
    // assert!(tx_template_values.code == tx_code);
    // let transactions = ledger
    //     .transactions()
    //     .list_by_external_ids(vec![external_id.clone()])
    //     .await?;
    // assert_eq!(transactions.len(), 1);

    // let entries = ledger
    //     .entries()
    //     .list_by_transaction_ids(vec![transactions[0].id])
    //     .await?;

    // assert!(entries.get(&transactions[0].id).is_some());
    // assert_eq!(entries.get(&transactions[0].id).unwrap().len(), 4);

    // assert_eq!(
    //     sender_account_balance_events.recv().await.unwrap().r#type,
    //     SqlxLedgerEventType::BalanceUpdated
    // );
    // let next_event = all_events.recv().await.unwrap();
    // assert_eq!(next_event.r#type, SqlxLedgerEventType::TransactionCreated);
    // assert_eq!(
    //     all_events.recv().await.unwrap().r#type,
    //     SqlxLedgerEventType::BalanceUpdated
    // );
    // let after_events = ledger
    //     .events(EventSubscriberOpts {
    //         after_id: Some(next_event.id),
    //         ..Default::default()
    //     })
    //     .await?;
    // assert_eq!(
    //     after_events.all().unwrap().recv().await.unwrap().r#type,
    //     SqlxLedgerEventType::BalanceUpdated
    // );
    // assert_eq!(
    //     all_events.recv().await.unwrap().r#type,
    //     SqlxLedgerEventType::BalanceUpdated
    // );
    // assert_eq!(
    //     journal_events.recv().await.unwrap().r#type,
    //     SqlxLedgerEventType::TransactionCreated
    // );
    // assert_eq!(
    //     journal_events.recv().await.unwrap().r#type,
    //     SqlxLedgerEventType::BalanceUpdated
    // );
    // assert_eq!(
    //     journal_events.recv().await.unwrap().r#type,
    //     SqlxLedgerEventType::BalanceUpdated
    // );

    // let usd = rusty_money::iso::find("USD").unwrap();
    // let btc = rusty_money::crypto::find("BTC").unwrap();

    // let usd_credit_balance = get_balance(
    //     &ledger,
    //     journal_id,
    //     recipient_account_id,
    //     Currency::Iso(usd),
    // )
    // .await?;
    // assert_eq!(usd_credit_balance.settled(), Decimal::from(100));

    // let btc_credit_balance = get_balance(
    //     &ledger,
    //     journal_id,
    //     recipient_account_id,
    //     Currency::Crypto(btc),
    // )
    // .await?;
    // assert_eq!(btc_credit_balance.settled(), Decimal::from(1290));

    // let btc_debit_balance = get_balance(
    //     &ledger,
    //     journal_id,
    //     sender_account_id,
    //     Currency::Crypto(btc),
    // )
    // .await?;
    // assert_eq!(btc_debit_balance.settled(), Decimal::from(-1290));

    // let usd_credit_balance =
    //     get_balance(&ledger, journal_id, sender_account_id, Currency::Iso(usd)).await?;
    // assert_eq!(usd_credit_balance.settled(), Decimal::from(-100));

    // let external_id = uuid::Uuid::new_v4().to_string();
    // params = TxParams::new();
    // params.insert("journal_id", journal_id);
    // params.insert("sender", sender_account_id);
    // params.insert("recipient", recipient_account_id);
    // params.insert("external_id", external_id.clone());

    // ledger
    //     .post_transaction(TransactionId::new(), &tx_code, Some(params))
    //     .await
    //     .unwrap();

    // let usd_credit_balance = get_balance(
    //     &ledger,
    //     journal_id,
    //     recipient_account_id,
    //     Currency::Iso(usd),
    // )
    // .await?;
    // assert_eq!(usd_credit_balance.settled(), Decimal::from(200));

    // let btc_credit_balance = get_balance(
    //     &ledger,
    //     journal_id,
    //     recipient_account_id,
    //     Currency::Crypto(btc),
    // )
    // .await?;
    // assert_eq!(btc_credit_balance.settled(), Decimal::from(2580));

    // let btc_debit_balance = get_balance(
    //     &ledger,
    //     journal_id,
    //     sender_account_id,
    //     Currency::Crypto(btc),
    // )
    // .await?;
    // assert_eq!(btc_debit_balance.settled(), Decimal::from(-2580));

    // let usd_credit_balance =
    //     get_balance(&ledger, journal_id, sender_account_id, Currency::Iso(usd)).await?;
    // assert_eq!(usd_credit_balance.settled(), Decimal::from(-200));

    // Ok(())
    Ok(())
}

// async fn get_balance(
//     ledger: &SqlxLedger,
//     journal_id: JournalId,
//     account_id: AccountId,
//     currency: Currency,
// ) -> anyhow::Result<AccountBalance> {
//     let balance = ledger
//         .balances()
//         .find(journal_id, account_id, currency)
//         .await?
//         .unwrap();
//     Ok(balance)
// }
