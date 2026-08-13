use rand::RngExt;
use rust_decimal::Decimal;

use cala_ledger::{account::*, journal::*, migrate::IncludeMigrations, tx_template::*, *};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let random_number = rand::rng().random_range(0..1000);
    let example_suffix = std::env::var("EXAMPLE_SUFFIX").unwrap_or(format!("{random_number:03}"));

    let pg_con = std::env::var("PG_CON")
        .unwrap_or_else(|_| "postgres://user:password@localhost:5432/pg".to_string());
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(20)
        .connect(&pg_con)
        .await?;
    sqlx::migrate!()
        .include_cala_migrations()
        .run(&pool)
        .await?;
    // The example never polls jobs; the EC rollup stays dormant.
    let mut jobs = job::Jobs::init(
        job::JobSvcConfig::builder()
            .pool(pool.clone())
            .build()
            .map_err(anyhow::Error::msg)?,
    )
    .await?;
    let cala_config = CalaLedgerConfig::builder()
        .pool(pool)
        .exec_migrations(false)
        .build()?;
    let cala = CalaLedger::init(cala_config, &mut jobs).await?;

    // Create two accounts so we have somewhere to move funds between.
    let new_sender = NewAccount::builder()
        .id(AccountId::new())
        .name(format!("SENDER #{random_number:03}"))
        .code(format!("SENDER.{example_suffix}"))
        .description("sender account")
        .build()?;
    let mut sender = cala.accounts().create(new_sender).await?;
    println!("sender_account_id: {}", sender.id());

    let new_recipient = NewAccount::builder()
        .id(AccountId::new())
        .name(format!("RECIPIENT #{random_number:03}"))
        .code(format!("RECIPIENT.{example_suffix}"))
        .description("recipient account")
        .build()?;
    let recipient = cala.accounts().create(new_recipient).await?;
    println!("recipient_account_id: {}", recipient.id());

    // Demonstrate the update flow: rename the sender and re-persist.
    let mut sender_update = AccountUpdate::default();
    sender_update
        .name(format!("SENDER #{random_number:04}"))
        .description("updated description")
        .build()?;
    if sender.update(sender_update).did_execute() {
        cala.accounts().persist(&mut sender).await?;
    }
    let sender = cala.accounts().find(sender.id()).await?;
    println!("sender name after update: {}", sender.values().name);

    let journal_id = JournalId::new();
    let new_journal = NewJournal::builder()
        .id(journal_id)
        .name("MY JOURNAL")
        .description("description")
        .build()?;
    let mut journal = cala.journals().create(new_journal).await?;
    println!("journal_id: {journal_id}");

    let mut journal_update = JournalUpdate::default();
    journal_update
        .name("UPDATED_JOURNAL_NAME")
        .description("new description")
        .build()?;
    if journal.update(journal_update).did_execute() {
        cala.journals().persist(&mut journal).await?;
    }

    // Templates are CEL expressions parameterized by `params.*`. The
    // ParamDataType-typed definitions below are what the caller must
    // supply when posting a transaction against this template.
    let template_params = vec![
        NewParamDefinition::builder()
            .name("sender")
            .r#type(ParamDataType::Uuid)
            .build()?,
        NewParamDefinition::builder()
            .name("recipient")
            .r#type(ParamDataType::Uuid)
            .build()?,
        NewParamDefinition::builder()
            .name("amount")
            .r#type(ParamDataType::Decimal)
            .build()?,
    ];
    let entries = vec![
        NewTxTemplateEntry::builder()
            .entry_type("'TEST_DR'")
            .account_id("params.sender")
            .layer("'SETTLED'")
            .direction("'DEBIT'")
            .units("params.amount")
            .currency("'BTC'")
            .build()?,
        NewTxTemplateEntry::builder()
            .entry_type("'TEST_CR'")
            .account_id("params.recipient")
            .layer("'SETTLED'")
            .direction("'CREDIT'")
            .units("params.amount")
            .currency("'BTC'")
            .build()?,
    ];
    let tx_code = format!("CODE_{example_suffix}");
    let new_tx_template = NewTxTemplate::builder()
        .id(TxTemplateId::new())
        .code(&tx_code)
        .params(template_params)
        .transaction(
            NewTxTemplateTransaction::builder()
                .journal_id(format!("uuid('{journal_id}')"))
                .effective("date()")
                .build()?,
        )
        .entries(entries)
        .build()?;
    let tx_template = cala.tx_templates().create(new_tx_template).await?;
    println!("tx_template_code: {}", tx_template.values().code);

    // Post a transaction against the template.
    let mut post_params = Params::new();
    post_params.insert("sender", sender.id());
    post_params.insert("recipient", recipient.id());
    post_params.insert("amount", Decimal::from(1290));

    cala.post_transaction(TransactionId::new(), &tx_code, post_params)
        .await?;

    let currency = "BTC".parse()?;
    let sender_balance = cala
        .balances()
        .find(journal_id, sender.id(), currency)
        .await?;
    let recipient_balance = cala
        .balances()
        .find(journal_id, recipient.id(), currency)
        .await?;
    println!("sender settled BTC: {}", sender_balance.settled());
    println!("recipient settled BTC: {}", recipient_balance.settled());

    Ok(())
}
