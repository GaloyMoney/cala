use derive_builder::Builder;
use es_entity::clock::{Clock, ClockHandle};

#[derive(Builder, Clone, Debug)]
#[builder(build_fn(validate = "Self::validate"))]
pub struct CalaLedgerConfig {
    #[builder(setter(into, strip_option), default)]
    pub(super) pg_con: Option<String>,
    #[builder(setter(into, strip_option), default)]
    pub(super) max_connections: Option<u32>,
    #[builder(default)]
    pub(super) exec_migrations: bool,
    #[builder(setter(into, strip_option), default)]
    pub(super) pool: Option<sqlx::PgPool>,
    /// Bound on how many `post_transaction*` calls may be in flight at
    /// once. Postings serialize on hot shared accounts via advisory
    /// locks; without a bound the queue forms inside Postgres where
    /// each waiter pins a connection, an open transaction and a
    /// snapshot. With a bound, excess posters wait in-process instead.
    /// `None` (default) keeps the previous unbounded behavior.
    #[builder(setter(into, strip_option), default)]
    pub(super) max_concurrent_postings: Option<usize>,
    #[builder(setter(into), default = "Clock::handle().clone()")]
    pub(super) clock: ClockHandle,
}

impl CalaLedgerConfig {
    pub fn builder() -> CalaLedgerConfigBuilder {
        CalaLedgerConfigBuilder::default()
    }
}

impl CalaLedgerConfigBuilder {
    fn validate(&self) -> Result<(), String> {
        match (self.pg_con.as_ref(), self.pool.as_ref()) {
            (None, None) | (Some(None), None) | (None, Some(None)) => {
                return Err("One of pg_con or pool must be set".to_string())
            }
            (Some(_), Some(_)) => return Err("Only one of pg_con or pool must be set".to_string()),
            _ => (),
        }
        Ok(())
    }
}
