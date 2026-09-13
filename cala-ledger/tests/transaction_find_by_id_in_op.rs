mod helpers;

use rand::distr::{Alphanumeric, SampleString};

use cala_ledger::{
    transaction::{error::TransactionError, Transaction},
    tx_template::*,
    *,
};

#[tokio::test]
async fn find_by_id_in_op_sees_uncommitted_write_pool_read_does_not() -> anyhow::Result<()> {
    let pool = helpers::init_pool().await?;
    let mut jobs = helpers::init_jobs(pool.clone()).await?;
    let cala_config = CalaLedgerConfig::builder()
        .pool(pool.clone())
        .exec_migrations(false)
        .build()?;
    let cala = CalaLedger::init(cala_config, &mut jobs).await?;

    let new_journal = helpers::test_journal();
    let journal = cala.journals().create(new_journal).await?;

    let (sender, recipient) = helpers::test_accounts();
    let sender_account = cala.accounts().create(sender).await?;
    let recipient_account = cala.accounts().create(recipient).await?;

    let tx_code = Alphanumeric.sample_string(&mut rand::rng(), 32);
    let new_template = helpers::currency_conversion_template(&tx_code);
    cala.tx_templates().create(new_template).await?;

    let mut params = Params::new();
    params.insert("journal_id", journal.id());
    params.insert("sender", sender_account.id());
    params.insert("recipient", recipient_account.id());

    let tx_id = TransactionId::new();

    let mut op = cala.begin_operation().await?;
    let posted = cala
        .post_transaction_in_op(&mut op, tx_id, &tx_code, params)
        .await?;
    assert_eq!(posted.id(), tx_id);

    match cala.transactions().find_by_id(tx_id).await {
        Err(TransactionError::CouldNotFindById(err_id)) => assert_eq!(err_id, tx_id),
        Err(other) => panic!("expected CouldNotFindById before commit, got err: {other}"),
        Ok(_) => panic!("expected CouldNotFindById before commit, got Ok"),
    }

    let seen_in_op = cala.transactions().find_by_id_in_op(&mut op, tx_id).await?;
    assert_eq!(seen_in_op.id(), tx_id);

    op.commit().await?;

    let seen_after_commit = cala.transactions().find_by_id(tx_id).await?;
    assert_eq!(seen_after_commit.id(), tx_id);

    let seen_via_pool = cala.transactions().find_by_id_in_op(&pool, tx_id).await?;
    assert_eq!(seen_via_pool.id(), tx_id);

    Ok(())
}

#[tokio::test]
async fn find_all_in_op_sees_uncommitted_write() -> anyhow::Result<()> {
    let pool = helpers::init_pool().await?;
    let mut jobs = helpers::init_jobs(pool.clone()).await?;
    let cala_config = CalaLedgerConfig::builder()
        .pool(pool)
        .exec_migrations(false)
        .build()?;
    let cala = CalaLedger::init(cala_config, &mut jobs).await?;

    let new_journal = helpers::test_journal();
    let journal = cala.journals().create(new_journal).await?;

    let (sender, recipient) = helpers::test_accounts();
    let sender_account = cala.accounts().create(sender).await?;
    let recipient_account = cala.accounts().create(recipient).await?;

    let tx_code = Alphanumeric.sample_string(&mut rand::rng(), 32);
    let new_template = helpers::currency_conversion_template(&tx_code);
    cala.tx_templates().create(new_template).await?;

    let mut params = Params::new();
    params.insert("journal_id", journal.id());
    params.insert("sender", sender_account.id());
    params.insert("recipient", recipient_account.id());

    let tx_id = TransactionId::new();

    let mut op = cala.begin_operation().await?;
    cala.post_transaction_in_op(&mut op, tx_id, &tx_code, params)
        .await?;

    let found: std::collections::HashMap<TransactionId, Transaction> = cala
        .transactions()
        .find_all_in_op(&mut op, &[tx_id])
        .await?;
    assert!(found.contains_key(&tx_id));

    op.commit().await?;

    Ok(())
}
