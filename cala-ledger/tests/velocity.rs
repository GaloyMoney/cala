mod helpers;

use rand::distributions::{Alphanumeric, DistString};
use rust_decimal::Decimal;

use cala_ledger::{velocity::*, *};

#[tokio::test]
async fn create_control() -> anyhow::Result<()> {
    let pool = helpers::init_pool().await?;
    let cala_config = CalaLedgerConfig::builder()
        .pool(pool)
        .exec_migrations(false)
        .build()?;
    let cala = CalaLedger::init(cala_config).await?;

    let new_journal = helpers::test_journal();
    let journal = cala.journals().create(new_journal).await.unwrap();

    let velocity = cala.velocities();

    let withdrawal_limit = NewVelocityLimit::builder()
        .id(VelocityLimitId::new())
        .name("Withdrawal")
        .description("test")
        .window(vec![])
        .currency(None)
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
        .currency(None)
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

    let (sender, receiver) = helpers::test_accounts();
    let sender_account = cala.accounts().create(sender).await.unwrap();
    let recipient_account = cala.accounts().create(receiver).await.unwrap();

    let mut params = Params::new();
    params.insert("withdrawal_limit", Decimal::from(100));
    params.insert("deposit_limit", Decimal::from(100));
    velocity
        .attach_control_to_account(control.id(), sender_account.id(), params)
        .await?;

    let tx_code = Alphanumeric.sample_string(&mut rand::thread_rng(), 32);
    let new_template = helpers::test_template(&tx_code);

    cala.tx_templates().create(new_template).await.unwrap();

    let mut params = Params::new();
    params.insert("journal_id", journal.id().to_string());
    params.insert("sender", sender_account.id());
    params.insert("recipient", recipient_account.id());

    cala.post_transaction(TransactionId::new(), &tx_code, params)
        .await
        .unwrap();

    Ok(())
}
