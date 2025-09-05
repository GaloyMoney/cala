pub mod templates;

use rand::distr::{Alphanumeric, SampleString};

use cala_ledger::{account::*, journal::*, velocity::*, *};

pub async fn init_pool() -> anyhow::Result<sqlx::PgPool> {
    let pg_host = std::env::var("PG_HOST").unwrap_or("localhost".to_string());
    let pg_con = format!("postgres://user:password@{pg_host}:5432/pg");
    let pool = sqlx::PgPool::connect(&pg_con).await?;
    Ok(pool)
}

pub async fn init_cala() -> anyhow::Result<CalaLedger> {
    let pool = init_pool().await?;
    let cala_config = CalaLedgerConfig::builder()
        .pool(pool)
        .exec_migrations(false)
        .build()?;
    let ledger = CalaLedger::init(cala_config).await?;
    Ok(ledger)
}

pub async fn init_journal(cala: &CalaLedger) -> anyhow::Result<Journal> {
    let name = Alphanumeric.sample_string(&mut rand::rng(), 32);
    let new_journal = NewJournal::builder()
        .id(JournalId::new())
        .name(name)
        .build()
        .unwrap();
    let journal = cala.journals().create(new_journal).await?;
    Ok(journal)
}

pub async fn init_accounts(
    cala: &CalaLedger,
    check_velocity: bool,
) -> anyhow::Result<(Account, Account)> {
    let code = Alphanumeric.sample_string(&mut rand::rng(), 32);
    let sender_account = NewAccount::builder()
        .id(uuid::Uuid::now_v7())
        .name(format!("Test Sender Account {code}"))
        .code(code)
        .metadata(serde_json::json!({"check_velocity": check_velocity}))
        .unwrap()
        .build()
        .unwrap();

    let code = Alphanumeric.sample_string(&mut rand::rng(), 32);
    let recipient_account = NewAccount::builder()
        .id(uuid::Uuid::now_v7())
        .name(format!("Test Recipient Account {code}"))
        .code(code)
        .metadata(serde_json::json!({"check_velocity": check_velocity}))
        .unwrap()
        .build()
        .unwrap();
    let sender = cala.accounts().create(sender_account).await?;
    let recipient = cala.accounts().create(recipient_account).await?;
    Ok((sender, recipient))
}

async fn create_velocity_control(
    cala: &CalaLedger,
    limit: rust_decimal::Decimal,
) -> anyhow::Result<(VelocityControlId, Params)> {
    let velocity = cala.velocities();

    let velocity_limit = NewVelocityLimit::builder()
        .id(VelocityLimitId::new())
        .name("Transfer Limit")
        .description("Benchmark velocity limit that never blocks")
        .window(vec![])
        .limit(
            NewLimit::builder()
                .balance(vec![
                    NewBalanceLimit::builder()
                        .layer("SETTLED")
                        .amount("params.transfer_limit")
                        .enforcement_direction("DEBIT")
                        .build()
                        .unwrap(),
                ])
                .build()
                .unwrap(),
        )
        .params(vec![
            NewParamDefinition::builder()
                .r#type(ParamDataType::Decimal)
                .name("transfer_limit")
                .build()
                .unwrap(),
        ])
        .build()
        .unwrap();

    let velocity_limit = velocity.create_limit(velocity_limit).await?;

    let control = NewVelocityControl::builder()
        .id(VelocityControlId::new())
        .name("Transfer Control")
        .description("Benchmark velocity control")
        .condition("context.vars.account.metadata.check_velocity")
        .build()
        .unwrap();
    let control = velocity.create_control(control).await?;

    velocity
        .add_limit_to_control(control.id(), velocity_limit.id())
        .await?;

    let mut control_params = Params::new();
    control_params.insert("transfer_limit", limit);

    Ok((control.id(), control_params))
}

pub async fn attach_velocity_to_account(
    cala: &CalaLedger,
    account_id: AccountId,
    limit: impl Into<rust_decimal::Decimal>,
) -> anyhow::Result<()> {
    let (control_id, control_params) = create_velocity_control(cala, limit.into()).await?;

    cala.velocities()
        .attach_control_to_account(control_id, account_id, control_params)
        .await?;

    Ok(())
}
