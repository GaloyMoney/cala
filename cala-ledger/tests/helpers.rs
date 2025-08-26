#![allow(dead_code)]
use rand::distr::{Alphanumeric, SampleString};

use cala_ledger::{account::*, journal::*, tx_template::*};

pub async fn init_pool() -> anyhow::Result<sqlx::PgPool> {
    let pg_host = std::env::var("PG_HOST").unwrap_or("localhost".to_string());
    let pg_con = format!("postgres://user:password@{pg_host}:5432/pg");
    let pool = sqlx::PgPool::connect(&pg_con).await?;
    Ok(pool)
}

pub fn test_journal() -> NewJournal {
    let name = Alphanumeric.sample_string(&mut rand::rng(), 32);
    NewJournal::builder()
        .id(JournalId::new())
        .name(name)
        .build()
        .unwrap()
}

pub fn test_journal_with_effective_balances() -> NewJournal {
    let name = Alphanumeric.sample_string(&mut rand::rng(), 32);
    NewJournal::builder()
        .id(JournalId::new())
        .name(name)
        .enable_effective_balance(true)
        .build()
        .unwrap()
}

pub fn test_accounts() -> (NewAccount, NewAccount) {
    let code = Alphanumeric.sample_string(&mut rand::rng(), 32);
    let sender_account = NewAccount::builder()
        .id(uuid::Uuid::now_v7())
        .name(format!("Test Sender Account {code}"))
        .code(code)
        .build()
        .unwrap();
    let code = Alphanumeric.sample_string(&mut rand::rng(), 32);
    let recipient_account = NewAccount::builder()
        .id(uuid::Uuid::now_v7())
        .name(format!("Test Recipient Account {code}"))
        .code(code)
        .build()
        .unwrap();
    (sender_account, recipient_account)
}

pub fn currency_conversion_template(code: &str) -> NewTxTemplate {
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
            .name("effective")
            .r#type(ParamDataType::Date)
            .default_expr("date()")
            .build()
            .unwrap(),
    ];
    let entries = vec![
        NewTxTemplateEntry::builder()
            .entry_type("'TEST_BTC_DR'")
            .account_id("params.sender")
            .layer("SETTLED")
            .direction("DEBIT")
            .units("decimal('1290')")
            .currency("'BTC'")
            .metadata(r#"{"sender": params.sender}"#)
            .build()
            .unwrap(),
        NewTxTemplateEntry::builder()
            .entry_type("'TEST_BTC_CR'")
            .account_id("params.recipient")
            .layer("SETTLED")
            .direction("CREDIT")
            .units("decimal('1290')")
            .currency("'BTC'")
            .build()
            .unwrap(),
        NewTxTemplateEntry::builder()
            .entry_type("'TEST_USD_DR'")
            .account_id("params.sender")
            .layer("SETTLED")
            .direction("DEBIT")
            .units("decimal('100')")
            .currency("'USD'")
            .build()
            .unwrap(),
        NewTxTemplateEntry::builder()
            .entry_type("'TEST_USD_CR'")
            .account_id("params.recipient")
            .layer("SETTLED")
            .direction("CREDIT")
            .units("decimal('100')")
            .currency("'USD'")
            .build()
            .unwrap(),
        NewTxTemplateEntry::builder()
            .entry_type("'TEST_USD_PENDING_DR'")
            .account_id("params.sender")
            .layer("PENDING")
            .direction("DEBIT")
            .units("decimal('100')")
            .currency("'USD'")
            .build()
            .unwrap(),
        NewTxTemplateEntry::builder()
            .entry_type("'TEST_USD_PENDING_CR'")
            .account_id("params.recipient")
            .layer("PENDING")
            .direction("CREDIT")
            .units("decimal('100')")
            .currency("'USD'")
            .build()
            .unwrap(),
    ];
    NewTxTemplate::builder()
        .id(uuid::Uuid::now_v7())
        .code(code)
        .params(params)
        .transaction(
            NewTxTemplateTransaction::builder()
                .effective("params.effective")
                .journal_id("params.journal_id")
                .metadata(r#"{"foo": "bar"}"#)
                .build()
                .unwrap(),
        )
        .entries(entries)
        .build()
        .unwrap()
}

pub fn velocity_template(code: &str) -> NewTxTemplate {
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
            .name("amount")
            .r#type(ParamDataType::Decimal)
            .build()
            .unwrap(),
        NewParamDefinition::builder()
            .name("currency")
            .r#type(ParamDataType::String)
            .default_expr("'USD'")
            .build()
            .unwrap(),
        NewParamDefinition::builder()
            .name("layer")
            .r#type(ParamDataType::String)
            .default_expr("'SETTLED'")
            .build()
            .unwrap(),
        NewParamDefinition::builder()
            .name("meta")
            .r#type(ParamDataType::Json)
            .default_expr(r#"{"foo": "bar"}"#)
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
            .entry_type("'TEST_DR'")
            .account_id("params.sender")
            .layer("params.layer")
            .direction("DEBIT")
            .units("params.amount")
            .currency("params.currency")
            .build()
            .unwrap(),
        NewTxTemplateEntry::builder()
            .entry_type("'TEST_CR'")
            .account_id("params.recipient")
            .layer("params.layer")
            .direction("CREDIT")
            .units("params.amount")
            .currency("params.currency")
            .build()
            .unwrap(),
    ];
    NewTxTemplate::builder()
        .id(uuid::Uuid::now_v7())
        .code(code)
        .params(params)
        .transaction(
            NewTxTemplateTransaction::builder()
                .effective("params.effective")
                .journal_id("params.journal_id")
                .metadata("params.meta")
                .build()
                .unwrap(),
        )
        .entries(entries)
        .build()
        .unwrap()
}
