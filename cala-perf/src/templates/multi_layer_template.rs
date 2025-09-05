use cala_ledger::{
    tx_template::{error::TxTemplateError, *},
    *,
};

pub const MULTI_LAYER_TEMPLATE_CODE: &str = "MULTI_LAYER_TEMPLATE";

pub async fn execute(
    cala: &CalaLedger,
    journal_id: JournalId,
    sender_id: AccountId,
    recipient_id: AccountId,
) -> anyhow::Result<()> {
    let mut params = Params::new();
    params.insert("journal_id", journal_id);
    params.insert("sender_id", sender_id);
    params.insert("recipient_id", recipient_id);

    cala.post_transaction(TransactionId::new(), MULTI_LAYER_TEMPLATE_CODE, params)
        .await?;
    Ok(())
}

pub async fn init(cala: &CalaLedger) -> anyhow::Result<()> {
    let params = vec![
        NewParamDefinition::builder()
            .name("recipient_id")
            .r#type(ParamDataType::Uuid)
            .build()
            .unwrap(),
        NewParamDefinition::builder()
            .name("sender_id")
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
            .entry_type("'MULTI_BTC_DR'")
            .account_id("params.sender_id")
            .layer("SETTLED")
            .direction("DEBIT")
            .units("decimal('1290')")
            .currency("'BTC'")
            .metadata(r#"{"sender_id": params.sender_id}"#)
            .build()
            .unwrap(),
        NewTxTemplateEntry::builder()
            .entry_type("'MULTI_BTC_CR'")
            .account_id("params.recipient_id")
            .layer("SETTLED")
            .direction("CREDIT")
            .units("decimal('1290')")
            .currency("'BTC'")
            .build()
            .unwrap(),
        NewTxTemplateEntry::builder()
            .entry_type("'MULTI_USD_DR'")
            .account_id("params.sender_id")
            .layer("SETTLED")
            .direction("DEBIT")
            .units("decimal('100')")
            .currency("'USD'")
            .build()
            .unwrap(),
        NewTxTemplateEntry::builder()
            .entry_type("'MULTI_USD_CR'")
            .account_id("params.recipient_id")
            .layer("SETTLED")
            .direction("CREDIT")
            .units("decimal('100')")
            .currency("'USD'")
            .build()
            .unwrap(),
        NewTxTemplateEntry::builder()
            .entry_type("'MULTI_USD_PENDING_DR'")
            .account_id("params.sender_id")
            .layer("PENDING")
            .direction("DEBIT")
            .units("decimal('100')")
            .currency("'USD'")
            .build()
            .unwrap(),
        NewTxTemplateEntry::builder()
            .entry_type("'MULTI_USD_PENDING_CR'")
            .account_id("params.recipient_id")
            .layer("PENDING")
            .direction("CREDIT")
            .units("decimal('100')")
            .currency("'USD'")
            .build()
            .unwrap(),
    ];
    let template = NewTxTemplate::builder()
        .id(uuid::Uuid::now_v7())
        .code(MULTI_LAYER_TEMPLATE_CODE)
        .params(params)
        .transaction(
            NewTxTemplateTransaction::builder()
                .effective("params.effective")
                .journal_id("params.journal_id")
                .metadata(r#"{"template": "multi_layer"}"#)
                .build()
                .unwrap(),
        )
        .entries(entries)
        .build()
        .unwrap();
    match cala.tx_templates().create(template).await {
        Err(TxTemplateError::DuplicateCode) => Ok(()),
        Err(e) => Err(e.into()),
        Ok(_) => Ok(()),
    }
}
