use anyhow::Context;
use rand::Rng;
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
    let random_number = rand::thread_rng().gen_range(0..1000);
    let example_suffix = std::env::var("EXAMPLE_SUFFIX").unwrap_or(format!("{:03}", random_number));

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
    let code_string = format!("USERS.{}", example_suffix);
    let new_account = NewAccount::builder()
        .id(AccountId::new())
        .name(format!("ACCOUNT #{:03}", random_number))
        .code(code_string)
        .description("description".to_string())
        .build()?;
    let mut account = cala.accounts().create(new_account).await?;
    println!("account_id: {}", account.id());

    // update account name and description
    let mut builder = AccountUpdate::default();
    builder
        .name(format!("ACCOUNT #{:03}", random_number))
        .description("new description".to_string())
        .build()?;
    account.update(builder);
    cala.accounts().persist(&mut account).await?;

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
        .journal_id(format!("uuid('{}')", journal_id))
        .effective("date('2022-11-01')")
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
        .code(format!("CODE_{}", example_suffix))
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
