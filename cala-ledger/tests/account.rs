mod helpers;

use cala_ledger::{account::error::AccountError, *};

#[tokio::test]
async fn find_returns_not_found_by_id() -> anyhow::Result<()> {
    let pool = helpers::init_pool().await?;
    let cala_config = CalaLedgerConfig::builder()
        .pool(pool)
        .exec_migrations(false)
        .build()?;
    let cala = CalaLedger::init(cala_config).await?;

    let id = AccountId::new();
    match cala.accounts().find(id).await {
        Err(AccountError::CouldNotFindById(err_id)) => assert_eq!(err_id, id),
        Err(other) => panic!("expected CouldNotFindById({id}), got: {other}"),
        Ok(_) => panic!("expected not-found error, got Ok"),
    }

    Ok(())
}
