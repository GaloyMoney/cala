use cala_ledger::{account::*, journal::*, migrate::IncludeMigrations, query::*, *};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let pg_host = std::env::var("PG_HOST").unwrap_or("localhost".to_string());
    let pg_con = format!("postgres://user:password@{pg_host}:5432/pg");
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
    println!("journal_id: {}", journal.id());

    Ok(())
}
