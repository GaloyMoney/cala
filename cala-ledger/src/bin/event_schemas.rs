#![cfg(feature = "json-schema")]

use std::{env, fs, path::PathBuf};

use cala_ledger::account::AccountEvent;
use schemars::schema_for;

struct SchemaInfo {
    name: &'static str,
    filename: &'static str,
    generate: fn() -> schemars::Schema,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let out_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("schemas");
    fs::create_dir_all(&out_dir)?;

    let schemas: &[SchemaInfo] = &[SchemaInfo {
        name: "AccountEvent",
        filename: "account_event_schema.json",
        generate: || schema_for!(AccountEvent),
    }];

    for schema in schemas {
        let path = out_dir.join(schema.filename);
        let json = serde_json::to_string_pretty(&(schema.generate)())?;
        fs::write(&path, format!("{json}\n"))?;
        println!("Wrote {} schema to {}", schema.name, path.display());
    }

    Ok(())
}
