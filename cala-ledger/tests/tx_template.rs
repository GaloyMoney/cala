mod helpers;

use rand::distr::{Alphanumeric, SampleString};
use rust_decimal::Decimal;

use cala_ledger::{error::LedgerError, tx_template::error::TxTemplateError, tx_template::*, *};

#[tokio::test]
async fn duplicate_code() -> anyhow::Result<()> {
    let pool = helpers::init_pool().await?;
    let mut jobs = helpers::init_jobs(pool.clone()).await?;
    let cala_config = CalaLedgerConfig::builder()
        .pool(pool)
        .exec_migrations(false)
        .build()?;
    let cala = CalaLedger::init(cala_config, &mut jobs).await?;

    let new_template = helpers::currency_conversion_template("tx_template_code");
    let _ = cala.tx_templates().create(new_template).await;

    let new_template = helpers::currency_conversion_template("tx_template_code");
    let res = cala.tx_templates().create(new_template).await;
    assert!(matches!(res, Err(TxTemplateError::DuplicateCode(_))));

    Ok(())
}

#[tokio::test]
async fn errors_on_non_positive_units() -> anyhow::Result<()> {
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
            .name("amount")
            .r#type(ParamDataType::Decimal)
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
            .units("params.amount")
            .currency("'USD'")
            .build()
            .unwrap(),
        NewTxTemplateEntry::builder()
            .entry_type("'CR'")
            .account_id("params.recipient")
            .layer("SETTLED")
            .direction("CREDIT")
            .units("params.amount")
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

    let make_params = |amount: Decimal| {
        let mut params = Params::new();
        params.insert("journal_id", journal.id());
        params.insert("sender", sender_account.id());
        params.insert("recipient", recipient_account.id());
        params.insert("amount", amount);
        params
    };

    // Negative units would invert the entry's accounting meaning and
    // must be rejected
    let res = cala
        .post_transaction(
            TransactionId::new(),
            &tx_code,
            make_params(Decimal::from(-100)),
        )
        .await;
    assert!(matches!(res, Err(LedgerError::TxTemplateError(
        TxTemplateError::NonPositiveUnits(_, _)
    ))));

    // Zero units are rejected too
    let res = cala
        .post_transaction(TransactionId::new(), &tx_code, make_params(Decimal::ZERO))
        .await;
    assert!(matches!(res, Err(LedgerError::TxTemplateError(
        TxTemplateError::NonPositiveUnits(_, _)
    ))));

    // Positive units still post
    let res = cala
        .post_transaction(
            TransactionId::new(),
            &tx_code,
            make_params(Decimal::from(100)),
        )
        .await;
    assert!(res.is_ok());

    Ok(())
}
