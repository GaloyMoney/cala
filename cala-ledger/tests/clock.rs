mod helpers;

use chrono::{TimeZone, Utc};
use es_entity::clock::{ArtificialClockConfig, ClockHandle};
use rand::distr::{Alphanumeric, SampleString};
use rust_decimal::Decimal;

use cala_ledger::{tx_template::*, *};

#[tokio::test]
async fn transaction_effective_date_uses_clock() -> anyhow::Result<()> {
    let (clock_handle, clock_ctrl) = ClockHandle::artificial(ArtificialClockConfig::manual());

    let fixed_time = Utc.with_ymd_and_hms(2025, 6, 15, 10, 30, 0).unwrap();
    clock_ctrl.set_time(fixed_time);

    let pool = helpers::init_pool().await?;
    let cala = CalaLedger::init(
        CalaLedgerConfig::builder()
            .pool(pool)
            .exec_migrations(false)
            .clock(clock_handle)
            .build()?,
    )
    .await?;

    let journal = cala.journals().create(helpers::test_journal()).await?;
    let (sender, recipient) = helpers::test_accounts();
    let sender = cala.accounts().create(sender).await?;
    let recipient = cala.accounts().create(recipient).await?;

    let tx_code = Alphanumeric.sample_string(&mut rand::rng(), 32);
    let template = helpers::simple_template_with_date_default(&tx_code);
    cala.tx_templates().create(template).await?;

    let mut params = Params::new();
    params.insert("journal_id", journal.id().to_string());
    params.insert("sender", sender.id());
    params.insert("recipient", recipient.id());
    params.insert("amount", Decimal::from(100));

    let tx = cala
        .post_transaction(TransactionId::new(), &tx_code, params)
        .await?;

    assert_eq!(tx.values().effective, fixed_time.date_naive());

    Ok(())
}

#[tokio::test]
async fn clock_advancement_changes_effective_date() -> anyhow::Result<()> {
    let (clock_handle, clock_ctrl) = ClockHandle::artificial(ArtificialClockConfig::manual());

    let time_1 = Utc.with_ymd_and_hms(2025, 1, 1, 12, 0, 0).unwrap();
    clock_ctrl.set_time(time_1);

    let pool = helpers::init_pool().await?;
    let cala = CalaLedger::init(
        CalaLedgerConfig::builder()
            .pool(pool)
            .exec_migrations(false)
            .clock(clock_handle)
            .build()?,
    )
    .await?;

    let journal = cala.journals().create(helpers::test_journal()).await?;
    let (sender, recipient) = helpers::test_accounts();
    let sender = cala.accounts().create(sender).await?;
    let recipient = cala.accounts().create(recipient).await?;

    let tx_code = Alphanumeric.sample_string(&mut rand::rng(), 32);
    let template = helpers::simple_template_with_date_default(&tx_code);
    cala.tx_templates().create(template).await?;

    let mut params = Params::new();
    params.insert("journal_id", journal.id().to_string());
    params.insert("sender", sender.id());
    params.insert("recipient", recipient.id());
    params.insert("amount", Decimal::from(50));

    let tx1 = cala
        .post_transaction(TransactionId::new(), &tx_code, params)
        .await?;

    let time_2 = Utc.with_ymd_and_hms(2025, 12, 25, 14, 0, 0).unwrap();
    clock_ctrl.set_time(time_2);

    let mut params = Params::new();
    params.insert("journal_id", journal.id().to_string());
    params.insert("sender", sender.id());
    params.insert("recipient", recipient.id());
    params.insert("amount", Decimal::from(75));

    let tx2 = cala
        .post_transaction(TransactionId::new(), &tx_code, params)
        .await?;

    assert_eq!(tx1.values().effective, time_1.date_naive());
    assert_eq!(tx2.values().effective, time_2.date_naive());
    assert_ne!(tx1.values().effective, tx2.values().effective);

    Ok(())
}

#[tokio::test]
async fn void_transaction_uses_clock_time() -> anyhow::Result<()> {
    let (clock_handle, clock_ctrl) = ClockHandle::artificial(ArtificialClockConfig::manual());

    let original_time = Utc.with_ymd_and_hms(2025, 3, 1, 9, 0, 0).unwrap();
    clock_ctrl.set_time(original_time);

    let pool = helpers::init_pool().await?;
    let cala = CalaLedger::init(
        CalaLedgerConfig::builder()
            .pool(pool)
            .exec_migrations(false)
            .clock(clock_handle)
            .build()?,
    )
    .await?;

    let journal = cala.journals().create(helpers::test_journal()).await?;
    let (sender, recipient) = helpers::test_accounts();
    let sender = cala.accounts().create(sender).await?;
    let recipient = cala.accounts().create(recipient).await?;

    let tx_code = Alphanumeric.sample_string(&mut rand::rng(), 32);
    let template = helpers::simple_template_with_date_default(&tx_code);
    cala.tx_templates().create(template).await?;

    let mut params = Params::new();
    params.insert("journal_id", journal.id().to_string());
    params.insert("sender", sender.id());
    params.insert("recipient", recipient.id());
    params.insert("amount", Decimal::from(100));

    let original_tx_id = TransactionId::new();
    let original_tx = cala
        .post_transaction(original_tx_id, &tx_code, params)
        .await?;

    let void_time = Utc.with_ymd_and_hms(2025, 3, 15, 16, 30, 0).unwrap();
    clock_ctrl.set_time(void_time);

    let voiding_tx_id = TransactionId::new();
    let voided_tx = cala.void_transaction(voiding_tx_id, original_tx_id).await?;

    assert_eq!(original_tx.created_at(), original_time);
    assert_eq!(voided_tx.created_at(), void_time);

    Ok(())
}

#[tokio::test]
async fn begin_operation_attaches_clock_time() -> anyhow::Result<()> {
    let (clock_handle, clock_ctrl) = ClockHandle::artificial(ArtificialClockConfig::manual());

    let fixed_time = Utc.with_ymd_and_hms(2025, 7, 4, 12, 0, 0).unwrap();
    clock_ctrl.set_time(fixed_time);

    let pool = helpers::init_pool().await?;
    let cala = CalaLedger::init(
        CalaLedgerConfig::builder()
            .pool(pool)
            .exec_migrations(false)
            .clock(clock_handle)
            .build()?,
    )
    .await?;

    let op = cala.begin_operation().await?;
    let op_time = op.now();

    assert_eq!(op_time, fixed_time);

    Ok(())
}

#[tokio::test]
async fn clock_propagates_through_atomic_operations() -> anyhow::Result<()> {
    let (clock_handle, clock_ctrl) = ClockHandle::artificial(ArtificialClockConfig::manual());

    let fixed_time = Utc.with_ymd_and_hms(2025, 9, 21, 8, 15, 0).unwrap();
    clock_ctrl.set_time(fixed_time);

    let pool = helpers::init_pool().await?;
    let cala = CalaLedger::init(
        CalaLedgerConfig::builder()
            .pool(pool)
            .exec_migrations(false)
            .clock(clock_handle)
            .build()?,
    )
    .await?;

    let journal = cala.journals().create(helpers::test_journal()).await?;
    let (sender, recipient) = helpers::test_accounts();
    let sender = cala.accounts().create(sender).await?;
    let recipient = cala.accounts().create(recipient).await?;

    let tx_code = Alphanumeric.sample_string(&mut rand::rng(), 32);
    let template = helpers::simple_template_with_date_default(&tx_code);
    cala.tx_templates().create(template).await?;

    let mut op = cala.begin_operation().await?;

    let mut params = Params::new();
    params.insert("journal_id", journal.id().to_string());
    params.insert("sender", sender.id());
    params.insert("recipient", recipient.id());
    params.insert("amount", Decimal::from(200));

    let tx = cala
        .post_transaction_in_op(&mut op, TransactionId::new(), &tx_code, params)
        .await?;

    op.commit().await?;

    assert_eq!(tx.created_at(), fixed_time);
    assert_eq!(tx.values().effective, fixed_time.date_naive());

    Ok(())
}
