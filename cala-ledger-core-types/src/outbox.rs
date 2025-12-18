use serde::{Deserialize, Serialize};

use crate::{
    account::*, account_set::*, balance::*, entry::*, journal::*, primitives::*, transaction::*,
    tx_template::*,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[allow(clippy::large_enum_variant)]
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
        member_id: AccountSetMemberId,
    },
    AccountSetMemberRemoved {
        source: DataSource,
        account_set_id: AccountSetId,
        member_id: AccountSetMemberId,
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
    TransactionUpdated {
        source: DataSource,
        transaction: TransactionValues,
        fields: Vec<String>,
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
