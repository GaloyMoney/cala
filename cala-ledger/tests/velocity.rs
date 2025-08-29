mod helpers;

use rand::distr::{Alphanumeric, SampleString};
use rust_decimal::Decimal;

use cala_ledger::{
    account::NewAccount,
    account_set::NewAccountSet,
    error::LedgerError,
    velocity::{error::VelocityError, *},
    *,
};

async fn control_and_limits(
    velocity: &Velocities,
    limit: Decimal,
) -> anyhow::Result<(VelocityControlId, Params)> {
    let withdrawal_limit = NewVelocityLimit::builder()
        .id(VelocityLimitId::new())
        .name("Withdrawal")
        .description("test")
        .window(vec![])
        .limit(
            NewLimit::builder()
                .balance(vec![NewBalanceLimit::builder()
                    .layer("SETTLED")
                    .amount("params.withdrawal_limit")
                    .enforcement_direction("DEBIT")
                    .build()
                    .expect("limit")])
                .build()
                .expect("limit"),
        )
        .params(vec![NewParamDefinition::builder()
            .r#type(ParamDataType::Decimal)
            .name("withdrawal_limit")
            .build()
            .expect("param")])
        .build()
        .expect("build limit");

    let withdrawal_limit = velocity.create_limit(withdrawal_limit).await?;

    let deposit_limit = NewVelocityLimit::builder()
        .id(VelocityLimitId::new())
        .name("Deposit")
        .description("test")
        .window(vec![])
        .limit(
            NewLimit::builder()
                .balance(vec![NewBalanceLimit::builder()
                    .layer("SETTLED")
                    .amount("params.deposit_limit")
                    .enforcement_direction("DEBIT")
                    .build()
                    .expect("limit")])
                .build()
                .expect("limit"),
        )
        .params(vec![NewParamDefinition::builder()
            .r#type(ParamDataType::Decimal)
            .name("deposit_limit")
            .build()
            .expect("param")])
        .build()
        .expect("build limit");
    let deposit_limit = velocity.create_limit(deposit_limit).await?;

    let control = NewVelocityControl::builder()
        .id(VelocityControlId::new())
        .name("test")
        .description("test")
        .build()
        .expect("build control");
    let control = velocity.create_control(control).await?;

    velocity
        .add_limit_to_control(control.id(), withdrawal_limit.id())
        .await?;
    velocity
        .add_limit_to_control(control.id(), deposit_limit.id())
        .await?;

    let mut control_params = Params::new();
    control_params.insert("withdrawal_limit", limit);
    control_params.insert("deposit_limit", limit);

    Ok((control.id(), control_params))
}

async fn account_closing_limit(
    velocity: &Velocities,
    direction: &'static str,
) -> anyhow::Result<VelocityLimit> {
    let new_limit = NewVelocityLimit::builder()
        .id(VelocityLimitId::new())
        .name("Account Closed")
        .description("Ensures no transactions allowed before cutoff date")
        .window(vec![])
        .limit(
            NewLimit::builder()
                .balance(vec![NewBalanceLimit::builder()
                    .layer("SETTLED")
                    .amount("decimal('0')")
                    .enforcement_direction(direction)
                    .build()
                    .expect("limit")])
                .build()
                .expect("limit"),
        )
        .params(vec![])
        .build()
        .expect("build limit");

    Ok(velocity.create_limit(new_limit).await?)
}

fn effective_date(y: i32, m: u32, d: u32) -> chrono::NaiveDate {
    chrono::NaiveDate::from_ymd_opt(y, m, d).unwrap()
}

#[tokio::test]
async fn create_control_on_account() -> anyhow::Result<()> {
    let pool = helpers::init_pool().await?;
    let cala_config = CalaLedgerConfig::builder()
        .pool(pool)
        .exec_migrations(false)
        .build()?;
    let cala = CalaLedger::init(cala_config).await?;

    let new_journal = helpers::test_journal();
    let journal = cala.journals().create(new_journal).await.unwrap();

    let velocity = cala.velocities();

    let limit = Decimal::ONE_HUNDRED;
    let (control_id, control_params) = control_and_limits(velocity, limit).await?;

    let tx_code = Alphanumeric.sample_string(&mut rand::rng(), 32);
    let new_template = helpers::velocity_template(&tx_code);
    cala.tx_templates().create(new_template).await.unwrap();

    let (sender, receiver) = helpers::test_accounts();
    let sender_account = cala.accounts().create(sender).await.unwrap();
    let recipient_account = cala.accounts().create(receiver).await.unwrap();
    velocity
        .attach_control_to_account(control_id, sender_account.id(), control_params.clone())
        .await?;

    let mut tx_params = Params::new();
    tx_params.insert("journal_id", journal.id().to_string());
    tx_params.insert("sender", sender_account.id());
    tx_params.insert("recipient", recipient_account.id());
    tx_params.insert("amount", limit);
    let _ = cala
        .post_transaction(TransactionId::new(), &tx_code, tx_params.clone())
        .await?;
    tx_params.insert("amount", Decimal::ONE);
    let res = cala
        .post_transaction(TransactionId::new(), &tx_code, tx_params.clone())
        .await;
    assert!(res.is_err());

    Ok(())
}

#[tokio::test]
async fn create_control_on_account_set() -> anyhow::Result<()> {
    let pool = helpers::init_pool().await?;
    let cala_config = CalaLedgerConfig::builder()
        .pool(pool)
        .exec_migrations(false)
        .build()?;
    let cala = CalaLedger::init(cala_config).await?;

    let new_journal = helpers::test_journal();
    let journal = cala.journals().create(new_journal).await.unwrap();

    let velocity = cala.velocities();

    let limit = Decimal::ONE_HUNDRED;
    let (control_id, control_params) = control_and_limits(velocity, limit).await?;

    let tx_code = Alphanumeric.sample_string(&mut rand::rng(), 32);
    let new_template = helpers::velocity_template(&tx_code);
    cala.tx_templates().create(new_template).await.unwrap();

    let (sender, receiver) = helpers::test_accounts();
    let sender_account = cala.accounts().create(sender).await.unwrap();
    let recipient_account = cala.accounts().create(receiver).await.unwrap();
    let (new_sender_account_set, new_recipient_account_set) =
        helpers::test_account_sets(journal.id().into());
    let sender_account_set = cala
        .account_sets()
        .create(new_sender_account_set)
        .await
        .unwrap();
    let recipient_account_set = cala
        .account_sets()
        .create(new_recipient_account_set)
        .await
        .unwrap();
    cala.account_sets()
        .add_member(sender_account_set.id, sender_account.id)
        .await
        .unwrap();
    cala.account_sets()
        .add_member(recipient_account_set.id, recipient_account.id)
        .await
        .unwrap();
    velocity
        .attach_control_to_account_set(control_id, sender_account_set.id(), control_params)
        .await?;

    let mut tx_params = Params::new();
    tx_params.insert("journal_id", journal.id().to_string());
    tx_params.insert("sender", sender_account.id());
    tx_params.insert("recipient", recipient_account.id());
    tx_params.insert("amount", limit);
    let _ = cala
        .post_transaction(TransactionId::new(), &tx_code, tx_params.clone())
        .await?;
    tx_params.insert("amount", Decimal::ONE);
    let res = cala
        .post_transaction(TransactionId::new(), &tx_code, tx_params.clone())
        .await;
    assert!(res.is_err());

    Ok(())
}

#[tokio::test]
async fn limit_children_accounts_via_grandparent_account() -> anyhow::Result<()> {
    let pool = helpers::init_pool().await?;
    let cala_config = CalaLedgerConfig::builder()
        .pool(pool)
        .exec_migrations(false)
        .build()?;
    let cala = CalaLedger::init(cala_config).await?;

    let new_journal = helpers::test_journal();
    let journal = cala.journals().create(new_journal).await.unwrap();

    let tx_code = Alphanumeric.sample_string(&mut rand::rng(), 32);
    let new_template = helpers::velocity_template(&tx_code);
    cala.tx_templates().create(new_template).await.unwrap();

    let velocity = cala.velocities();

    let debit_limit = account_closing_limit(velocity, "DEBIT").await?;
    let credit_limit = account_closing_limit(velocity, "CREDIT").await?;

    let control = NewVelocityControl::builder()
        .id(VelocityControlId::new())
        .name("Account Closing")
        .description("test")
        .condition("context.vars.transaction.effective <= date('2024-12-31')")
        .build()
        .expect("build control");

    let control = velocity.create_control(control).await?;
    velocity
        .add_limit_to_control(control.id(), debit_limit.id())
        .await?;
    velocity
        .add_limit_to_control(control.id(), credit_limit.id())
        .await?;

    // Setup account sets and accounts in dag
    let code = Alphanumeric.sample_string(&mut rand::rng(), 32);
    let new_parent_account_set = NewAccountSet::builder()
        .id(uuid::Uuid::now_v7())
        .name(format!("Test Parent Account Set {code}"))
        .journal_id(journal.id())
        .build()
        .unwrap();
    let parent_account_set = cala
        .account_sets()
        .create(new_parent_account_set)
        .await
        .unwrap();
    velocity
        .attach_control_to_account_set(control.id(), parent_account_set.id(), Params::new())
        .await?;

    let code = Alphanumeric.sample_string(&mut rand::rng(), 32);
    let new_child_account_set = NewAccountSet::builder()
        .id(uuid::Uuid::now_v7())
        .name(format!("Test Child Account Set {code}"))
        .journal_id(journal.id())
        .build()
        .unwrap();
    let child_account_set = cala
        .account_sets()
        .create(new_child_account_set)
        .await
        .unwrap();
    cala.account_sets()
        .add_member(parent_account_set.id(), child_account_set.id())
        .await?;

    let code = Alphanumeric.sample_string(&mut rand::rng(), 32);
    let new_account_1 = NewAccount::builder()
        .id(uuid::Uuid::now_v7())
        .name(format!("Test Sender Account {code}"))
        .code(code)
        .build()
        .unwrap();
    let account_1 = cala.accounts().create(new_account_1).await.unwrap();
    cala.account_sets()
        .add_member(child_account_set.id(), account_1.id())
        .await?;

    let code = Alphanumeric.sample_string(&mut rand::rng(), 32);
    let new_account_2 = NewAccount::builder()
        .id(uuid::Uuid::now_v7())
        .name(format!("Test Sender Account {code}"))
        .code(code)
        .build()
        .unwrap();
    let account_2 = cala.accounts().create(new_account_2).await.unwrap();
    cala.account_sets()
        .add_member(child_account_set.id(), account_2.id())
        .await?;

    let code = Alphanumeric.sample_string(&mut rand::rng(), 32);
    let new_open_account = NewAccount::builder()
        .id(uuid::Uuid::now_v7())
        .name(format!("Test Recipient Account {code}"))
        .code(code)
        .build()
        .unwrap();
    let open_account = cala.accounts().create(new_open_account).await.unwrap();

    // Execute transactions
    let mut tx_params = Params::new();
    tx_params.insert("journal_id", journal.id().to_string());
    tx_params.insert("recipient", open_account.id());
    tx_params.insert("amount", Decimal::ONE);

    tx_params.insert("sender", account_1.id());
    tx_params.insert("effective", effective_date(2025, 1, 1));
    let _ = cala
        .post_transaction(TransactionId::new(), &tx_code, tx_params.clone())
        .await?;
    tx_params.insert("sender", account_1.id());
    tx_params.insert("effective", effective_date(2024, 12, 31));
    let account_1_send_res = cala
        .post_transaction(TransactionId::new(), &tx_code, tx_params.clone())
        .await;
    assert!(matches!(
        account_1_send_res,
        Err(LedgerError::VelocityError(VelocityError::Enforcement(_)))
    ));

    tx_params.insert("sender", account_2.id());
    tx_params.insert("effective", effective_date(2025, 1, 1));
    let _ = cala
        .post_transaction(TransactionId::new(), &tx_code, tx_params.clone())
        .await?;
    tx_params.insert("sender", account_2.id());
    tx_params.insert("effective", effective_date(2024, 12, 31));
    let account_2_send_res = cala
        .post_transaction(TransactionId::new(), &tx_code, tx_params.clone())
        .await;
    assert!(matches!(
        account_2_send_res,
        Err(LedgerError::VelocityError(VelocityError::Enforcement(_)))
    ));

    // TODO: update closing date on account set and re-check

    Ok(())
}
