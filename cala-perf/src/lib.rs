pub mod templates;

use rand::distr::{Alphanumeric, SampleString};

use cala_ledger::{account::*, account_set::*, journal::*, velocity::*, *};

pub async fn init_pool() -> anyhow::Result<sqlx::PgPool> {
    let pg_host = std::env::var("PG_HOST").unwrap_or("localhost".to_string());
    let pg_con = format!("postgres://user:password@{pg_host}:5432/pg");

    // Configure pool for high-concurrency performance testing
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(90) // Increase max connections for concurrent testing
        .min_connections(10) // Keep minimum connections ready
        .acquire_timeout(std::time::Duration::from_secs(60)) // Increase timeout
        .idle_timeout(Some(std::time::Duration::from_secs(600))) // 10 min idle timeout
        .max_lifetime(Some(std::time::Duration::from_secs(3600))) // 1 hour max lifetime
        .connect(&pg_con)
        .await?;

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

pub async fn init_journal(cala: &CalaLedger, effective_balances: bool) -> anyhow::Result<Journal> {
    let name = Alphanumeric.sample_string(&mut rand::rng(), 32);
    let new_journal = NewJournal::builder()
        .id(JournalId::new())
        .name(name)
        .enable_effective_balance(effective_balances)
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
                .balance(vec![NewBalanceLimit::builder()
                    .layer("SETTLED")
                    .amount("params.transfer_limit")
                    .enforcement_direction("DEBIT")
                    .build()
                    .unwrap()])
                .build()
                .unwrap(),
        )
        .params(vec![NewParamDefinition::builder()
            .r#type(ParamDataType::Decimal)
            .name("transfer_limit")
            .build()
            .unwrap()])
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

pub async fn init_accounts_with_account_sets_depth(
    cala: &CalaLedger,
    journal: &Journal,
    check_velocity: bool,
    depth: usize,
) -> anyhow::Result<(Account, Account, AccountSet, AccountSet)> {
    let (sender, recipient) = init_accounts(cala, check_velocity).await?;

    // Create nested account sets for sender
    let mut sender_sets = Vec::new();
    for i in 0..depth {
        let sender_set = NewAccountSet::builder()
            .id(AccountSetId::new())
            .name(format!("Sender Account Set L{}", i + 1))
            .journal_id(journal.id())
            .build()
            .unwrap();
        let sender_set = cala.account_sets().create(sender_set).await?;
        sender_sets.push(sender_set);
    }

    // Create nested account sets for recipient
    let mut recipient_sets = Vec::new();
    for i in 0..depth {
        let recipient_set = NewAccountSet::builder()
            .id(AccountSetId::new())
            .name(format!("Recipient Account Set L{}", i + 1))
            .journal_id(journal.id())
            .build()
            .unwrap();
        let recipient_set = cala.account_sets().create(recipient_set).await?;
        recipient_sets.push(recipient_set);
    }

    // Build nested hierarchy for sender: set_0 contains set_1 contains ... contains account
    for i in 0..depth {
        if i == depth - 1 {
            // Innermost set contains the account
            cala.account_sets()
                .add_member(sender_sets[i].id(), sender.id())
                .await?;
        } else {
            // Set contains the next nested set
            cala.account_sets()
                .add_member(sender_sets[i].id(), sender_sets[i + 1].id())
                .await?;
        }
    }

    // Build nested hierarchy for recipient: set_0 contains set_1 contains ... contains account
    for i in 0..depth {
        if i == depth - 1 {
            // Innermost set contains the account
            cala.account_sets()
                .add_member(recipient_sets[i].id(), recipient.id())
                .await?;
        } else {
            // Set contains the next nested set
            cala.account_sets()
                .add_member(recipient_sets[i].id(), recipient_sets[i + 1].id())
                .await?;
        }
    }

    Ok((
        sender,
        recipient,
        sender_sets.remove(0),
        recipient_sets.remove(0),
    ))
}
