mod helpers;

use std::time::Duration;

use rand::distr::{Alphanumeric, SampleString};
use rust_decimal::Decimal;

use cala_ledger::{
    error::LedgerError,
    posting::{PostingError, RejectionReason},
    tx_template::*,
    velocity::{error::VelocityError, *},
    *,
};

#[tokio::test]
async fn blocks_transactions() -> anyhow::Result<()> {
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
        &res,
        Err(LedgerError::PostingError(PostingError::Rejected { reason, .. }))
            if matches!(reason.as_ref(), RejectionReason::AccountLocked(id) if *id == sender_account.id())
    ));

    Ok(())
}

#[tokio::test]
async fn posting_started_before_lock_can_commit_after_lock() -> anyhow::Result<()> {
    let pool = helpers::init_pool().await?;
    let mut jobs = helpers::init_jobs(pool.clone()).await?;
    let cala_config = CalaLedgerConfig::builder()
        .pool(pool.clone())
        .exec_migrations(false)
        .build()?;
    let cala = CalaLedger::init(cala_config, &mut jobs).await?;

    let journal = cala.journals().create(helpers::test_journal()).await?;
    let (sender, recipient) = helpers::test_accounts();
    let sender = cala.accounts().create(sender).await?;
    let recipient = cala.accounts().create(recipient).await?;

    let tx_code = Alphanumeric.sample_string(&mut rand::rng(), 32);
    cala.tx_templates()
        .create(helpers::velocity_template(&tx_code))
        .await?;

    let tx_id = TransactionId::new();
    let mut params = Params::new();
    params.insert("journal_id", journal.id());
    params.insert("sender", sender.id());
    params.insert("recipient", recipient.id());
    params.insert("amount", Decimal::from(100));

    let mut posting_op = cala.begin_operation().await?;
    cala.post_transaction_in_op(&mut posting_op, tx_id, &tx_code, params)
        .await?;

    let mut locking_op = cala.begin_operation().await?;
    cala.accounts()
        .lock_in_op(&mut locking_op, sender.id())
        .await?;
    locking_op.commit().await?;

    let locked = cala.accounts().find(sender.id()).await?;
    assert_eq!(
        locked.values().status,
        cala_types::primitives::Status::Locked
    );

    posting_op.commit().await?;

    assert_eq!(cala.transactions().find_by_id(tx_id).await?.id(), tx_id);
    assert_eq!(
        cala.balances()
            .find(journal.id(), sender.id(), "USD".parse()?)
            .await?
            .settled(),
        Decimal::from(-100)
    );

    Ok(())
}

#[tokio::test]
async fn surrender_blocks_concurrent_posting_at_zero_floor() -> anyhow::Result<()> {
    let pool = helpers::init_pool().await?;
    let mut jobs = helpers::init_jobs(pool.clone()).await?;
    let cala_config = CalaLedgerConfig::builder()
        .pool(pool.clone())
        .exec_migrations(false)
        .build()?;
    let cala = CalaLedger::init(cala_config, &mut jobs).await?;

    let journal = cala.journals().create(helpers::test_journal()).await?;
    let (facility, omnibus) = helpers::test_accounts();
    let (_, disbursal) = helpers::test_accounts();
    let facility = cala.accounts().create(facility).await?;
    let omnibus = cala.accounts().create(omnibus).await?;
    let disbursal = cala.accounts().create(disbursal).await?;

    let tx_code = Alphanumeric.sample_string(&mut rand::rng(), 32);
    cala.tx_templates()
        .create(helpers::velocity_template(&tx_code))
        .await?;

    let zero_floor = cala
        .velocities()
        .create_limit(
            NewVelocityLimit::builder()
                .id(VelocityLimitId::new())
                .name("zero floor")
                .description("settled balance cannot become debit")
                .window(vec![])
                .limit(
                    NewLimit::builder()
                        .balance(vec![NewBalanceLimit::builder()
                            .layer("SETTLED")
                            .amount("decimal('0')")
                            .enforcement_direction("DEBIT")
                            .always_active()
                            .build()?])
                        .build()?,
                )
                .build()?,
        )
        .await?;
    let control = cala
        .velocities()
        .create_control(
            NewVelocityControl::builder()
                .id(VelocityControlId::new())
                .name("facility commitment")
                .description("facility commitment")
                .build()?,
        )
        .await?;
    cala.velocities()
        .add_limit_to_control(control.id(), zero_floor.id())
        .await?;
    cala.velocities()
        .attach_control_to_account(control.id(), facility.id(), Params::new())
        .await?;

    let mut seed_params = Params::new();
    seed_params.insert("journal_id", journal.id());
    seed_params.insert("sender", omnibus.id());
    seed_params.insert("recipient", facility.id());
    seed_params.insert("amount", Decimal::from(100));
    cala.post_transaction(TransactionId::new(), &tx_code, seed_params)
        .await?;

    let mut surrender_params = Params::new();
    surrender_params.insert("journal_id", journal.id());
    surrender_params.insert("sender", facility.id());
    surrender_params.insert("recipient", omnibus.id());
    surrender_params.insert("amount", Decimal::from(100));
    let mut surrender_op = cala.begin_operation().await?;
    cala.post_transaction_in_op(
        &mut surrender_op,
        TransactionId::new(),
        &tx_code,
        surrender_params,
    )
    .await?;

    let concurrent_cala = cala.clone();
    let concurrent_code = tx_code.clone();
    let concurrent_journal_id = journal.id();
    let concurrent_facility_id = facility.id();
    let concurrent_disbursal_id = disbursal.id();
    let mut concurrent = tokio::spawn(async move {
        let mut params = Params::new();
        params.insert("journal_id", concurrent_journal_id);
        params.insert("sender", concurrent_facility_id);
        params.insert("recipient", concurrent_disbursal_id);
        params.insert("amount", Decimal::ONE);
        concurrent_cala
            .post_transaction(TransactionId::new(), &concurrent_code, params)
            .await
    });
    assert!(
        tokio::time::timeout(Duration::from_millis(300), &mut concurrent)
            .await
            .is_err(),
        "concurrent posting should wait for surrender's balance lock"
    );

    surrender_op.commit().await?;

    let err = match concurrent.await? {
        Err(err) => err,
        Ok(_) => panic!("posting should violate the zero floor after surrender commits"),
    };
    assert!(matches!(
        err,
        LedgerError::PostingError(PostingError::VelocityError(VelocityError::Enforcement(_)))
    ));
    assert_eq!(
        cala.balances()
            .find(journal.id(), facility.id(), "USD".parse()?)
            .await?
            .settled(),
        Decimal::ZERO
    );

    Ok(())
}
