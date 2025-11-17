mod helpers;

use rand::distr::{Alphanumeric, SampleString};
use rust_decimal::Decimal;

use cala_ledger::{balance::error::BalanceError, error::LedgerError, tx_template::*, *};

#[tokio::test]
async fn blocks_transactions() -> anyhow::Result<()> {
    let pool = helpers::init_pool().await?;
    let cala_config = CalaLedgerConfig::builder()
        .pool(pool.clone())
        .exec_migrations(false)
        .build()?;
    let cala = CalaLedger::init(cala_config).await?;

    let new_journal = helpers::test_journal();
    let journal = cala.journals().create(new_journal).await?;

    let (sender, recipient) = helpers::test_accounts();
    let sender_account = cala.accounts().create(sender).await?;
    let recipient_account = cala.accounts().create(recipient).await?;

    let mut op = cala.begin_operation().await?;
    let res = cala
        .accounts()
        .lock_in_op(&mut op, sender_account.id())
        .await;
    op.commit().await?;
    assert!(res.is_ok());

    let locked_account = cala.accounts().find(sender_account.id()).await?;
    assert_eq!(
        locked_account.values().status,
        cala_types::primitives::Status::Locked
    );

    let tx_code = Alphanumeric.sample_string(&mut rand::rng(), 32);
    let new_template = helpers::velocity_template(&tx_code);
    cala.tx_templates().create(new_template).await?;

    let mut params = Params::new();
    params.insert("journal_id", journal.id());
    params.insert("sender", sender_account.id());
    params.insert("recipient", recipient_account.id());
    params.insert("amount", Decimal::from(100));

    let res = cala
        .post_transaction(TransactionId::new(), &tx_code, params.clone())
        .await;
    assert!(matches!(
        res,
        Err(LedgerError::BalanceError(BalanceError::AccountLocked(_)))
    ));
    if let Err(LedgerError::BalanceError(BalanceError::AccountLocked(locked_id))) = res {
        assert_eq!(locked_id, sender_account.id());
    }

    Ok(())
}
