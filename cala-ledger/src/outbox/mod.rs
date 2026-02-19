mod publisher;

mod event {
    pub use cala_types::outbox::*;
}

pub use event::*;
pub use publisher::OutboxPublisher;

#[derive(Debug, obix::MailboxTables)]
#[obix(tbl_prefix = "cala")]
pub struct CalaMailboxTables;

pub type ObixOutbox = obix::Outbox<OutboxEventPayload, CalaMailboxTables>;
