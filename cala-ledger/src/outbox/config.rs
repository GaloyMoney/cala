use es_entity::clock::ClockHandle;

use std::sync::Arc;

use super::CalaMailboxTables;

pub const DEFAULT_OUTBOX_ARCHIVE_RETENTION_DAYS: u32 = 3;
pub const DEFAULT_OUTBOX_ARCHIVE_PATH_PREFIX: &str = "outbox-archive/cala/";

/// Cold-storage archiving of old outbox events (obix archive). Archived
/// history is swept to object storage and pruned from postgres; reads of
/// pre-watermark events transparently fall back to the archive.
///
/// The object-storage backend is supplied by the consumer via
/// [`obix::EventArchiveStorage`] (GCS, S3, local filesystem, ...);
/// [`obix::InMemoryArchiveStorage`] works for tests.
#[derive(Clone)]
pub struct OutboxArchiveConfig {
    /// The object-storage backend chunks are written to / read from.
    pub storage: Arc<dyn obix::EventArchiveStorage>,
    /// Days of history kept in postgres; older, fully-elapsed days are
    /// swept to storage one day per archiver run.
    pub retention_days: u32,
    /// Prepended to every chunk path, e.g. `"outbox-archive/cala/"`.
    pub path_prefix: String,
}

impl OutboxArchiveConfig {
    pub fn new(storage: Arc<dyn obix::EventArchiveStorage>) -> Self {
        Self {
            storage,
            retention_days: DEFAULT_OUTBOX_ARCHIVE_RETENTION_DAYS,
            path_prefix: DEFAULT_OUTBOX_ARCHIVE_PATH_PREFIX.to_string(),
        }
    }

    pub fn with_retention_days(mut self, retention_days: u32) -> Self {
        self.retention_days = retention_days;
        self
    }

    pub fn with_path_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.path_prefix = prefix.into();
        self
    }

    pub(super) fn build(&self, pool: &sqlx::PgPool, clock: &ClockHandle) -> obix::ArchiveConfig {
        obix::ArchiveConfig::new(
            self.storage.clone(),
            Arc::new(obix::DailyRetentionBoundary::<CalaMailboxTables>::new(
                pool,
                chrono::Duration::days(i64::from(self.retention_days)),
                clock.clone(),
            )),
        )
        .with_path_prefix(self.path_prefix.clone())
        .with_clock(clock.clone())
    }
}

impl std::fmt::Debug for OutboxArchiveConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OutboxArchiveConfig")
            .field("storage", &"<dyn EventArchiveStorage>")
            .field("retention_days", &self.retention_days)
            .field("path_prefix", &self.path_prefix)
            .finish()
    }
}
