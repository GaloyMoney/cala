mod helpers;

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

    let limit = NewVelocityLimit::builder()
        .id(VelocityLimitId::new())
        .name("Test")
        .description("test")
        .window(vec![])
        .currency(None)
        .limit(NewLimit::builder().balance(vec![]).build().expect("limit"))
        .build()
        .expect("build limit");

    let limit = velocity.create_limit(limit).await?;

    let control = NewVelocityControl::builder()
        .id(VelocityControlId::new())
        .name("test")
        .description("test")
        .build()
        .expect("build control");
    let control = velocity.create_control(control).await?;

    velocity
        .add_limit_to_control(control.id(), limit.id())
        .await?;
    Ok(())
}
