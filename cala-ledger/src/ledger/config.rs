use derive_builder::Builder;
use es_entity::clock::{Clock, ClockHandle};

use crate::outbox::OutboxArchiveConfig;

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
    #[builder(setter(into), default = "Clock::handle().clone()")]
    pub(super) clock: ClockHandle,
    /// Cold-storage archiving of old outbox events. When set, settled
    /// history is swept out of postgres by the archiver job (see
    /// [`CalaLedger::register_outbox_archiver`](crate::ledger::CalaLedger::register_outbox_archiver))
    /// and pre-watermark reads fall back to the archive.
    #[builder(setter(strip_option), default)]
    pub(super) outbox_archive: Option<OutboxArchiveConfig>,
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
