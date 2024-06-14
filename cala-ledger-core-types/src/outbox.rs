use serde::{Deserialize, Serialize};

use crate::{
    account::*, account_set::*, balance::*, entry::*, journal::*, primitives::*, transaction::*,
    tx_template::*,
};

#[derive(Debug, Serialize, Deserialize)]
pub struct OutboxEvent {
    pub id: OutboxEventId,
    pub sequence: EventSequence,
    pub payload: OutboxEventPayload,
    pub recorded_at: chrono::DateTime<chrono::Utc>,
}

impl Clone for OutboxEvent {
    fn clone(&self) -> Self {
        Self {
            id: self.id,
            sequence: self.sequence,
            payload: self.payload.clone(),
            recorded_at: self.recorded_at,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum OutboxEventPayload {
    Empty,
    AccountCreated {
        source: DataSource,
        account: AccountValues,
    },
    AccountUpdated {
        source: DataSource,
        account: AccountValues,
        fields: Vec<String>,
    },
    AccountSetCreated {
        source: DataSource,
        account_set: AccountSetValues,
    },
    AccountSetUpdated {
        source: DataSource,
        account_set: AccountSetValues,
        fields: Vec<String>,
    },
    AccountSetMemberCreated {
        source: DataSource,
        account_set_id: AccountSetId,
        member: AccountSetMember,
    },
    JournalCreated {
        source: DataSource,
        journal: JournalValues,
    },
    JournalUpdated {
        source: DataSource,
        journal: JournalValues,
        fields: Vec<String>,
    },
    TxTemplateCreated {
        source: DataSource,
        tx_template: TxTemplateValues,
    },
    TransactionCreated {
        source: DataSource,
        transaction: TransactionValues,
    },
    EntryCreated {
        source: DataSource,
        entry: EntryValues,
    },
    BalanceCreated {
        source: DataSource,
        balance: BalanceSnapshot,
    },
    BalanceUpdated {
        source: DataSource,
        balance: BalanceSnapshot,
    },
}

#[derive(
    sqlx::Type, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Copy, Clone, Serialize, Deserialize,
)]
#[serde(transparent)]
#[sqlx(transparent)]
pub struct EventSequence(i64);
impl EventSequence {
    pub const BEGIN: Self = EventSequence(0);
    pub fn next(&self) -> Self {
        Self(self.0 + 1)
    }
}

impl Default for EventSequence {
    fn default() -> Self {
        Self::BEGIN
    }
}

impl From<u64> for EventSequence {
    fn from(n: u64) -> Self {
        Self(n as i64)
    }
}

impl From<EventSequence> for u64 {
    fn from(EventSequence(n): EventSequence) -> Self {
        n as u64
    }
}

impl From<EventSequence> for std::sync::atomic::AtomicU64 {
    fn from(EventSequence(n): EventSequence) -> Self {
        std::sync::atomic::AtomicU64::new(n as u64)
    }
}
impl std::fmt::Display for EventSequence {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
