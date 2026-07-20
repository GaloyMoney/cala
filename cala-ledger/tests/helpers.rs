#![allow(dead_code)]
use rand::distr::{Alphanumeric, SampleString};

use cala_ledger::{
    account::*, account_set::NewAccountSet, job::*, journal::*, primitives::BalanceRollup,
    tx_template::*, AccountId, CalaLedger, Currency,
};

pub async fn init_pool() -> anyhow::Result<sqlx::PgPool> {
    init_pool_with(sqlx::postgres::PgPoolOptions::new()).await
}

/// Create a fresh, isolated database. The streaming EC-balance rollup job
/// is a **global** outbox consumer, so any test that runs it must not share
/// a database with other tests — otherwise it would roll their transactions
/// into its EC sets. cala's own migrations already provision the job +
/// obix tables, so a plain `migrate!().run()` suffices.
pub async fn init_isolated_pool() -> anyhow::Result<sqlx::PgPool> {
    use sqlx::Connection as _;

    let base = std::env::var("PG_CON")?;
    let db_name = format!("cala_ec_stream_{}", uuid::Uuid::now_v7().simple());

    let mut admin = sqlx::PgConnection::connect(&base).await?;
    let create = format!(r#"CREATE DATABASE "{db_name}""#);
    sqlx::query(&create).execute(&mut admin).await?;
    admin.close().await?;

    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(10)
        .connect(&with_db_name(&base, &db_name))
        .await?;
    sqlx::migrate!().run(&pool).await?;
    Ok(pool)
}

fn with_db_name(url: &str, db: &str) -> String {
    let (base, query) = match url.split_once('?') {
        Some((b, q)) => (b, Some(q)),
        None => (url, None),
    };
    let idx = base.rfind('/').expect("connection URL has a path segment");
    let mut out = format!("{}/{}", &base[..idx], db);
    if let Some(q) = query {
        out.push('?');
        out.push_str(q);
    }
    out
}

/// Build a `job::Jobs` on `pool`. The streaming rollup is registered inside
/// `CalaLedger::init` (pass `Some(&mut jobs)`); the test drives `start_poll`
/// itself — typically *after* posting its backlog. Keep the returned `Jobs`
/// alive for the duration of the test (dropping it shuts the poller down).
pub async fn init_jobs(pool: sqlx::PgPool) -> anyhow::Result<Jobs> {
    Ok(Jobs::init(
        JobSvcConfig::builder()
            .pool(pool)
            .build()
            .map_err(anyhow::Error::msg)?,
    )
    .await?)
}

/// Poll (up to ~30s) until `account_id`'s settled balance reaches `expected`.
pub async fn wait_for_settled(
    cala: &CalaLedger,
    journal_id: JournalId,
    account_id: impl Into<AccountId> + Copy + std::fmt::Debug,
    currency: Currency,
    expected: rust_decimal::Decimal,
) -> anyhow::Result<()> {
    for _ in 0..300 {
        if let Ok(bal) = cala.balances().find(journal_id, account_id, currency).await {
            if bal.settled() == expected {
                return Ok(());
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    let last = cala.balances().find(journal_id, account_id, currency).await;
    anyhow::bail!("settled balance did not converge to {expected}; last observed = {last:?}");
}

/// Poll (up to ~30s) until `account_id`'s cumulative effective settled
/// balance as of `date` reaches `expected`.
pub async fn wait_for_effective(
    cala: &CalaLedger,
    journal_id: JournalId,
    account_id: impl Into<AccountId> + Copy + std::fmt::Debug,
    currency: Currency,
    date: chrono::NaiveDate,
    expected: rust_decimal::Decimal,
) -> anyhow::Result<()> {
    for _ in 0..300 {
        if let Ok(bal) = cala
            .balances()
            .effective()
            .find_cumulative(journal_id, account_id, currency, date)
            .await
        {
            if bal.settled() == expected {
                return Ok(());
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    let last = cala
        .balances()
        .effective()
        .find_cumulative(journal_id, account_id, currency, date)
        .await;
    anyhow::bail!("effective balance did not converge to {expected}; last observed = {last:?}");
}

/// Same as `init_pool`, but lets the caller pre-configure the pool (max
/// connections, acquire timeout, etc.) for tests that need more headroom.
pub async fn init_pool_with(
    options: sqlx::postgres::PgPoolOptions,
) -> anyhow::Result<sqlx::PgPool> {
    let pg_con = std::env::var("PG_CON").unwrap();
    let pool = options.connect(&pg_con).await?;
    use job::IncludeMigrations;
    sqlx::migrate!().include_job_migrations().run(&pool).await?;
    Ok(pool)
}

pub fn test_journal() -> NewJournal {
    let name = Alphanumeric.sample_string(&mut rand::rng(), 32);
    NewJournal::builder()
        .id(JournalId::new())
        .name(name)
        .build()
        .unwrap()
}

pub fn test_journal_with_effective_balances() -> NewJournal {
    let name = Alphanumeric.sample_string(&mut rand::rng(), 32);
    NewJournal::builder()
        .id(JournalId::new())
        .name(name)
        .enable_effective_balance(true)
        .build()
        .unwrap()
}

pub fn test_accounts() -> (NewAccount, NewAccount) {
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
    (sender_account, recipient_account)
}

pub fn test_account_sets(journal_id: uuid::Uuid) -> (NewAccountSet, NewAccountSet) {
    let code = Alphanumeric.sample_string(&mut rand::rng(), 32);
    let sender_account_set = NewAccountSet::builder()
        .id(uuid::Uuid::now_v7())
        .name(format!("Test Sender Account Set {code}"))
        .journal_id(journal_id)
        .balance_rollup(BalanceRollup::Synchronous)
        .build()
        .unwrap();

    let code = Alphanumeric.sample_string(&mut rand::rng(), 32);
    let recipient_account_set = NewAccountSet::builder()
        .id(uuid::Uuid::now_v7())
        .name(format!("Test Recipient Account Set {code}"))
        .journal_id(journal_id)
        .balance_rollup(BalanceRollup::Synchronous)
        .build()
        .unwrap();

    (sender_account_set, recipient_account_set)
}

pub fn currency_conversion_template(code: &str) -> NewTxTemplate {
    let params = vec![
        NewParamDefinition::builder()
            .name("recipient")
            .r#type(ParamDataType::Uuid)
            .build()
            .unwrap(),
        NewParamDefinition::builder()
            .name("sender")
            .r#type(ParamDataType::Uuid)
            .build()
            .unwrap(),
        NewParamDefinition::builder()
            .name("journal_id")
            .r#type(ParamDataType::Uuid)
            .build()
            .unwrap(),
        NewParamDefinition::builder()
            .name("effective")
            .r#type(ParamDataType::Date)
            .default_expr("date()")
            .build()
            .unwrap(),
    ];
    let entries = vec![
        NewTxTemplateEntry::builder()
            .entry_type("'TEST_BTC_DR'")
            .account_id("params.sender")
            .layer("SETTLED")
            .direction("DEBIT")
            .units("decimal('1290')")
            .currency("'BTC'")
            .metadata(r#"{"sender": params.sender}"#)
            .build()
            .unwrap(),
        NewTxTemplateEntry::builder()
            .entry_type("'TEST_BTC_CR'")
            .account_id("params.recipient")
            .layer("SETTLED")
            .direction("CREDIT")
            .units("decimal('1290')")
            .currency("'BTC'")
            .build()
            .unwrap(),
        NewTxTemplateEntry::builder()
            .entry_type("'TEST_USD_DR'")
            .account_id("params.sender")
            .layer("SETTLED")
            .direction("DEBIT")
            .units("decimal('100')")
            .currency("'USD'")
            .build()
            .unwrap(),
        NewTxTemplateEntry::builder()
            .entry_type("'TEST_USD_CR'")
            .account_id("params.recipient")
            .layer("SETTLED")
            .direction("CREDIT")
            .units("decimal('100')")
            .currency("'USD'")
            .build()
            .unwrap(),
        NewTxTemplateEntry::builder()
            .entry_type("'TEST_USD_PENDING_DR'")
            .account_id("params.sender")
            .layer("PENDING")
            .direction("DEBIT")
            .units("decimal('100')")
            .currency("'USD'")
            .build()
            .unwrap(),
        NewTxTemplateEntry::builder()
            .entry_type("'TEST_USD_PENDING_CR'")
            .account_id("params.recipient")
            .layer("PENDING")
            .direction("CREDIT")
            .units("decimal('100')")
            .currency("'USD'")
            .build()
            .unwrap(),
    ];
    NewTxTemplate::builder()
        .id(uuid::Uuid::now_v7())
        .code(code)
        .params(params)
        .transaction(
            NewTxTemplateTransaction::builder()
                .effective("params.effective")
                .journal_id("params.journal_id")
                .metadata(r#"{"foo": "bar"}"#)
                .build()
                .unwrap(),
        )
        .entries(entries)
        .build()
        .unwrap()
}

pub fn simple_template_with_date_default(code: &str) -> NewTxTemplate {
    let params = vec![
        NewParamDefinition::builder()
            .name("recipient")
            .r#type(ParamDataType::Uuid)
            .build()
            .unwrap(),
        NewParamDefinition::builder()
            .name("sender")
            .r#type(ParamDataType::Uuid)
            .build()
            .unwrap(),
        NewParamDefinition::builder()
            .name("journal_id")
            .r#type(ParamDataType::Uuid)
            .build()
            .unwrap(),
        NewParamDefinition::builder()
            .name("amount")
            .r#type(ParamDataType::Decimal)
            .build()
            .unwrap(),
    ];
    let entries = vec![
        NewTxTemplateEntry::builder()
            .entry_type("'CLOCK_TEST_DR'")
            .account_id("params.sender")
            .layer("SETTLED")
            .direction("DEBIT")
            .units("params.amount")
            .currency("'USD'")
            .build()
            .unwrap(),
        NewTxTemplateEntry::builder()
            .entry_type("'CLOCK_TEST_CR'")
            .account_id("params.recipient")
            .layer("SETTLED")
            .direction("CREDIT")
            .units("params.amount")
            .currency("'USD'")
            .build()
            .unwrap(),
    ];
    NewTxTemplate::builder()
        .id(uuid::Uuid::now_v7())
        .code(code)
        .params(params)
        .transaction(
            NewTxTemplateTransaction::builder()
                .effective("date()")
                .journal_id("params.journal_id")
                .build()
                .unwrap(),
        )
        .entries(entries)
        .build()
        .unwrap()
}

pub fn velocity_template(code: &str) -> NewTxTemplate {
    let params = vec![
        NewParamDefinition::builder()
            .name("recipient")
            .r#type(ParamDataType::Uuid)
            .build()
            .unwrap(),
        NewParamDefinition::builder()
            .name("sender")
            .r#type(ParamDataType::Uuid)
            .build()
            .unwrap(),
        NewParamDefinition::builder()
            .name("journal_id")
            .r#type(ParamDataType::Uuid)
            .build()
            .unwrap(),
        NewParamDefinition::builder()
            .name("amount")
            .r#type(ParamDataType::Decimal)
            .build()
            .unwrap(),
        NewParamDefinition::builder()
            .name("currency")
            .r#type(ParamDataType::String)
            .default_expr("'USD'")
            .build()
            .unwrap(),
        NewParamDefinition::builder()
            .name("layer")
            .r#type(ParamDataType::String)
            .default_expr("'SETTLED'")
            .build()
            .unwrap(),
        NewParamDefinition::builder()
            .name("meta")
            .r#type(ParamDataType::Json)
            .default_expr(r#"{"foo": "bar"}"#)
            .build()
            .unwrap(),
        NewParamDefinition::builder()
            .name("effective")
            .r#type(ParamDataType::Date)
            .default_expr("date()")
            .build()
            .unwrap(),
    ];
    let entries = vec![
        NewTxTemplateEntry::builder()
            .entry_type("'TEST_DR'")
            .account_id("params.sender")
            .layer("params.layer")
            .direction("DEBIT")
            .units("params.amount")
            .currency("params.currency")
            .build()
            .unwrap(),
        NewTxTemplateEntry::builder()
            .entry_type("'TEST_CR'")
            .account_id("params.recipient")
            .layer("params.layer")
            .direction("CREDIT")
            .units("params.amount")
            .currency("params.currency")
            .build()
            .unwrap(),
    ];
    NewTxTemplate::builder()
        .id(uuid::Uuid::now_v7())
        .code(code)
        .params(params)
        .transaction(
            NewTxTemplateTransaction::builder()
                .effective("params.effective")
                .journal_id("params.journal_id")
                .metadata("params.meta")
                .build()
                .unwrap(),
        )
        .entries(entries)
        .build()
        .unwrap()
}
