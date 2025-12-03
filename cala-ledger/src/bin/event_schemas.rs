#![cfg(feature = "json-schema")]

use std::{env, fs, path::PathBuf};

use cala_ledger::{
    account::AccountEvent,
    account_set::AccountSetEvent,
    entry::EntryEvent,
    journal::JournalEvent,
    transaction::TransactionEvent,
    tx_template::TxTemplateEvent,
    velocity::{VelocityControlEvent, VelocityLimitEvent},
};
use schemars::schema_for;

struct SchemaInfo {
    name: &'static str,
    filename: &'static str,
    generate: fn() -> schemars::Schema,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let out_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("schemas");
    fs::create_dir_all(&out_dir)?;

    let schemas: &[SchemaInfo] = &[
        SchemaInfo {
            name: "AccountEvent",
            filename: "account_event_schema.json",
            generate: || schema_for!(AccountEvent),
        },
        SchemaInfo {
            name: "AccountSetEvent",
            filename: "account_set_event_schema.json",
            generate: || schema_for!(AccountSetEvent),
        },
        SchemaInfo {
            name: "JournalEvent",
            filename: "journal_event_schema.json",
            generate: || schema_for!(JournalEvent),
        },
        SchemaInfo {
            name: "TransactionEvent",
            filename: "transaction_event_schema.json",
            generate: || schema_for!(TransactionEvent),
        },
        SchemaInfo {
            name: "EntryEvent",
            filename: "entry_event_schema.json",
            generate: || schema_for!(EntryEvent),
        },
        SchemaInfo {
            name: "TxTemplateEvent",
            filename: "tx_template_event_schema.json",
            generate: || schema_for!(TxTemplateEvent),
        },
        SchemaInfo {
            name: "VelocityLimitEvent",
            filename: "velocity_limit_event_schema.json",
            generate: || schema_for!(VelocityLimitEvent),
        },
        SchemaInfo {
            name: "VelocityControlEvent",
            filename: "velocity_control_event_schema.json",
            generate: || schema_for!(VelocityControlEvent),
        },
    ];

    for schema in schemas {
        let path = out_dir.join(schema.filename);
        let json = serde_json::to_string_pretty(&(schema.generate)())?;
        fs::write(&path, format!("{json}\n"))?;
        println!("Wrote {} schema to {}", schema.name, path.display());
    }

    Ok(())
}
