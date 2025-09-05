pub mod templates;

use rand::distr::{Alphanumeric, SampleString};

use cala_ledger::{account::*, journal::*, *};

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

pub async fn init_accounts(cala: &CalaLedger) -> anyhow::Result<(Account, Account)> {
    let code = Alphanumeric.sample_string(&mut rand::rng(), 32);
    let sender_account = NewAccount::builder()
        .id(uuid::Uuid::now_v7())
        .name(format!("Test Sender Account {code}"))
        .code(code)
        .build()
        .unwrap();

    let code = Alphanumeric.sample_string(&mut rand::rng(), 32);
    let recipient_account = NewAccount::builder()
        .id(uuid::Uuid::now_v7())
        .name(format!("Test Recipient Account {code}"))
        .code(code)
        .build()
        .unwrap();
    let sender = cala.accounts().create(sender_account).await?;
    let recipient = cala.accounts().create(recipient_account).await?;
    Ok((sender, recipient))
}
