use es_entity::clock::{Clock, ClockHandle};
use es_entity::errlanes;

#[derive(Debug, errlanes::Rejection)]
pub enum InvalidCalaLedgerConfig {
    #[rejection(code = "CALA_LEDGER_CONFIG_NO_CONNECTION")]
    #[error("One of pg_con or pool must be set")]
    NoConnection,
    #[rejection(code = "CALA_LEDGER_CONFIG_BOTH_CONNECTIONS")]
    #[error("Only one of pg_con or pool must be set")]
    BothConnections,
}

#[derive(Clone, Debug)]
pub struct CalaLedgerConfig {
    pub(super) pg_con: Option<String>,
    pub(super) max_connections: Option<u32>,
    pub(super) exec_migrations: bool,
    pub(super) pool: Option<sqlx::PgPool>,
    pub(super) clock: ClockHandle,
}

#[bon::bon]
impl CalaLedgerConfig {
    #[builder]
    pub fn new(
        #[builder(into)] pg_con: Option<String>,
        #[builder(into)] max_connections: Option<u32>,
        #[builder(default)] exec_migrations: bool,
        #[builder(into)] pool: Option<sqlx::PgPool>,
        #[builder(into, default = Clock::handle().clone())] clock: ClockHandle,
    ) -> Result<Self, InvalidCalaLedgerConfig> {
        match (pg_con.as_ref(), pool.as_ref()) {
            (None, None) => return Err(InvalidCalaLedgerConfig::NoConnection),
            (Some(_), Some(_)) => return Err(InvalidCalaLedgerConfig::BothConnections),
            _ => (),
        }
        Ok(Self {
            pg_con,
            max_connections,
            exec_migrations,
            pool,
            clock,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn requires_exactly_one_connection_source() {
        assert!(matches!(
            CalaLedgerConfig::builder().build(),
            Err(InvalidCalaLedgerConfig::NoConnection)
        ));

        let pool = sqlx::PgPool::connect_lazy("postgres://localhost/unused").unwrap();
        assert!(matches!(
            CalaLedgerConfig::builder()
                .pg_con("postgres://localhost/unused")
                .pool(pool)
                .build(),
            Err(InvalidCalaLedgerConfig::BothConnections)
        ));
    }
}
