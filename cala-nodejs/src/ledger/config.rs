#[napi(object)]
pub struct OutboxServerConfig {
    pub enabled: bool,
    pub listen_port: Option<u16>,
}

#[napi(object)]
pub struct CalaLedgerConfig {
    pub pg_con: String,
    pub max_connections: Option<u32>,
    pub outbox: Option<OutboxServerConfig>,
}
