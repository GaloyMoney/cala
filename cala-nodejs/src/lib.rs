#![deny(clippy::all)]

#[macro_use]
extern crate napi_derive;

#[napi(object)]
pub struct CalaLedgerConfig {
    pub pg_con: String,
    pub max_connections: Option<u32>,
}

#[napi]
pub fn init(config: CalaLedgerConfig) {
    use cala_ledger::CalaLedgerConfig as Config;
    let mut builder = Config::builder();
    builder.pg_con(config.pg_con).exec_migrations(true);
    if let Some(n) = config.max_connections {
        builder.max_connections(n);
    }
    let config = builder.build().unwrap();
    println!("config: {:?}", config);
}
