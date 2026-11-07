use serde::{Deserialize, Serialize};

use crate::{
    account::*, account_set::*, calculation::*, entry::*, journal::*, primitives::*,
    transaction::*, tx_template::*,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[allow(clippy::large_enum_variant)]
pub enum OutboxEventPayload {
    Empty,
    AccountCreated {
        account: AccountValues,
    },
    AccountUpdated {
        account: AccountValues,
        fields: Vec<String>,
    },
    AccountSetCreated {
        account_set: AccountSetValues,
    },
    AccountSetUpdated {
        account_set: AccountSetValues,
        fields: Vec<String>,
    },
    AccountSetMemberCreated {
        account_set_id: AccountSetId,
        member_id: AccountSetMemberId,
    },
    AccountSetMemberRemoved {
        account_set_id: AccountSetId,
        member_id: AccountSetMemberId,
    },
    JournalCreated {
        journal: JournalValues,
    },
    JournalUpdated {
        journal: JournalValues,
        fields: Vec<String>,
    },
    TxTemplateCreated {
        tx_template: TxTemplateValues,
    },
    TransactionCreated {
        transaction: TransactionValues,
    },
    TransactionUpdated {
        transaction: TransactionValues,
        fields: Vec<String>,
    },
    EntryCreated {
        entry: EntryValues,
    },
    CalculationCreated {
        calculation: CalculationValues,
    },
    CalculationBackfillCompleted {
        calculation_id: CalculationId,
        /// `None` for a Global calculation; the attachment batch's targets otherwise.
        account_ids: Option<Vec<AccountId>>,
        activation_sequence: u64,
    },
}
