mod helpers;

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

    let (one, _) = helpers::test_accounts();
    let one = cala.accounts().create(one).await.unwrap();
    let mut params = Params::new();
    params.insert("withdrawal_limit", Decimal::from(100));
    params.insert("deposit_limit", Decimal::from(100));
    velocity
        .attach_control_to_account(control.id(), one.id(), params)
        .await?;
    Ok(())
}
