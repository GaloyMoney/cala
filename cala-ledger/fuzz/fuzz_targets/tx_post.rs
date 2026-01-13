#![no_main]

use futures;
use libfuzzer_sys::fuzz_target;
use once_cell::sync::Lazy;
use tokio::runtime::Runtime;
//use uuid::uuid;

#[path = "../../tests/helpers.rs"]
mod helpers;

use std::collections::HashMap;
//use std::hash::Hash;

use rand::distr::{Alphanumeric, SampleString};
//use rust_decimal::Decimal;

use cala_ledger::es_entity::TryFromEvents;
use cala_ledger::{entry::*, transaction::*, tx_template::*, *};

static RUNTIME: Lazy<Runtime> = Lazy::new(|| Runtime::new().unwrap());

struct FuzzData {
    data: Vec<String>,
}

impl FuzzData {
    fn new(bytes: &[u8]) -> Self {
        FuzzData {
            data: std::str::from_utf8(bytes)
                .unwrap()
                .split_whitespace()
                .map(|s| s.to_string())
                .collect(),
        }
    }

    fn len(&mut self) -> usize {
        self.data.len()
    }
}

pub async fn init_pool() -> anyhow::Result<sqlx::PgPool> {
    let pg_host = std::env::var("PG_HOST").unwrap_or("localhost".to_string());
    let pg_con = format!("postgres://user:password@{pg_host}:5432/pg");
    let pool = sqlx::PgPool::connect(&pg_con).await?;
    Ok(pool)
}

async fn transaction_post(fuzz_data: &FuzzData) -> anyhow::Result<()> {
    fn transaction() -> Transaction {
        let id = TransactionId::new();
        let values = TransactionValues {
            id,
            version: 1,
            created_at: chrono::Utc::now(),
            modified_at: chrono::Utc::now(),
            journal_id: JournalId::new(),
            tx_template_id: TxTemplateId::new(),
            entry_ids: vec![],
            effective: chrono::Utc::now().date_naive(),
            correlation_id: "correlation_id".to_string(),
            external_id: Some("external_id".to_string()),
            description: None,
            voided_by: None,
            void_of: None,
            metadata: Some(serde_json::json!({
                "tx": "metadata"
            })),
        };

        let events = es_entity::EntityEvents::init(id, [TransactionEvent::Initialized { values }]);
        Transaction::try_from_events(events).unwrap()
    }
    let default_tx = transaction();
    let default_entries: HashMap<EntryId, Entry> = HashMap::new();

    let pool = init_pool().await?;
    let cala_config = CalaLedgerConfig::builder()
        .pool(pool)
        .exec_migrations(false)
        .build()?;
    let cala = CalaLedger::init(cala_config).await?;

    let tx_code = Alphanumeric.sample_string(&mut rand::rng(), 32);
    let new_template = helpers::currency_conversion_template(&tx_code);

    cala.tx_templates().create(new_template).await.unwrap();

    let mut params = Params::new();

    // this is improvable as we need structured values such as Uuid, Date, etc,
    // for keywords such as journal_id, sender, effective. Otherwise we just hit
    // PG exceptions on later unwraps

    // println!("Transaction data:");
    for pair in fuzz_data.data.chunks(2) {
        if let [k, v] = pair {
            // println!("  {}: {}", k, v);
            params.insert(k.clone(), v.clone());
        }
    }

    let tx = cala
        .post_transaction(TransactionId::new(), &tx_code, params)
        .await
        //.unwrap();
        .unwrap_or(default_tx);

    let entries = cala
        .entries()
        .find_all(&tx.values().entry_ids)
        .await
        //.unwrap();
        .unwrap_or(default_entries);

    // print metadata in case we have some
    for entry in entries.values() {
        if let Some(metadata) = &entry.values().metadata {
            let metadata: HashMap<String, AccountId> =
                serde_json::from_value(metadata.clone()).unwrap();
            println!("{:#?}", metadata);
        }
    }
    Ok(())
}

fuzz_target!(|data: &[u8]| {
    let mut fuzz_data = FuzzData::new(data);

    // restrictions on fuzzed data
    if fuzz_data.len() < 8 {
        // not enough fields
        return;
    }

    for field in &fuzz_data.data {
        if field.len() < 4 && field != "id" {
            // any field which is too small (unless it's "id")
            return;
        }
    }

    // println!("{:#?}", fuzz_data.data);

    futures::executor::block_on(async move {
        RUNTIME
            .spawn(async move {
                transaction_post(&fuzz_data).await;
            })
            .await
            .unwrap()
    });

    // fuzzing iteration done
});
