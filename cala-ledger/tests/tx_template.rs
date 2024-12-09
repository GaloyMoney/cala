mod helpers;

use cala_ledger::{tx_template::error::TxTemplateError, *};

#[tokio::test]
async fn duplicate_code() -> anyhow::Result<()> {
    let pool = helpers::init_pool().await?;
    let cala_config = CalaLedgerConfig::builder()
        .pool(pool)
        .exec_migrations(false)
        .build()?;
    let cala = CalaLedger::init(cala_config).await?;

    let new_template = helpers::currency_conversion_template("tx_template_code");
    let _ = cala.tx_templates().create(new_template).await;

    let new_template = helpers::currency_conversion_template("tx_template_code");
    let res = cala.tx_templates().create(new_template).await;
    assert!(matches!(res, Err(TxTemplateError::DuplicateCode)));

    Ok(())
}
