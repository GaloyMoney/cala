use anyhow::Context;
use std::fs;

use cala_ledger::{
    account::*, journal::*, migrate::IncludeMigrations, query::*, tx_template::*, *,
};

pub fn store_server_pid(cala_home: &str, pid: u32) -> anyhow::Result<()> {
    create_cala_dir(cala_home)?;
    let _ = fs::remove_file(format!("{cala_home}/rust-example-pid"));
    fs::write(format!("{cala_home}/rust-example-pid"), pid.to_string())
        .context("Writing PID file")?;
    Ok(())
}

fn create_cala_dir(bria_home: &str) -> anyhow::Result<()> {
    let _ = fs::create_dir(bria_home);
    Ok(())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    store_server_pid(".cala", std::process::id())?;
    let pg_con = "postgres://user:password@localhost:5433/pg".to_string();
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(20)
        .connect(&pg_con)
        .await?;
    sqlx::migrate!()
        .include_cala_migrations()
        .run(&pool)
        .await?;
    let cala_config = CalaLedgerConfig::builder()
        .pool(pool)
        .exec_migrations(false)
        .outbox(OutboxServerConfig::default())
        .build()?;
    let cala = CalaLedger::init(cala_config).await?;
    let new_account = NewAccount::builder()
        .id(AccountId::new())
        .name("MY ACCOUNT")
        .code("USERS.abc")
        .description("description")
        .build()?;
    let account = cala.accounts().create(new_account).await?;
    println!("account_id: {}", account.id());

    let result = cala.accounts().list(PaginatedQueryArgs::default()).await?;
    println!("No of accounts: {}", result.entities.len());

    let new_journal = NewJournal::builder()
        .id(JournalId::new())
        .name("MY JOURNAL")
        .description("description")
        .build()?;
    let journal = cala.journals().create(new_journal).await?;
    let journal_id = journal.id();
    println!("journal_id: {}", journal_id);

    let tx_input = NewTxInput::builder()
        .journal_id(format!("UUID('{}')", journal_id))
        .effective("DATE('2022-11-01')")
        .build()?;
    let entries = vec![
        NewEntryInput::builder()
            .entry_type("'TEST_DR'")
            .account_id("param.recipient")
            .layer("'SETTLED'")
            .direction("'DEBIT'")
            .units("1290")
            .currency("'BTC'")
            .build()
            .unwrap(),
        NewEntryInput::builder()
            .entry_type("'TEST_CR'")
            .account_id("param.sender")
            .layer("'SETTLED'")
            .direction("'CREDIT'")
            .units("1290")
            .currency("'BTC'")
            .build()
            .unwrap(),
    ];
    let tx_template_id = TxTemplateId::new();

    let new_tx_template = NewTxTemplate::builder()
        .id(tx_template_id)
        .code("CODE")
        .tx_input(tx_input)
        .entries(entries)
        .build()
        .unwrap();
    let tx_template = cala.tx_templates().create(new_tx_template).await?;
    println!("tx_template_id: {}", tx_template.id());
    let code = tx_template.into_values().code;
    println!("tx_template_code: {}", code);

    loop {
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
    }
}
