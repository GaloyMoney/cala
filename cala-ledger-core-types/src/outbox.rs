use serde::{Deserialize, Serialize};

use crate::{account::*, account_set::*, balance::*, entry::*, primitives::*, transaction::*};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[allow(clippy::large_enum_variant)]
pub enum OutboxEventPayload {
    Empty,
    AccountCreated {
        account: AccountValues,
    },
    AccountSetMemberCreated {
        account_set_id: AccountSetId,
        member_id: AccountSetMemberId,
    },
    AccountSetMemberRemoved {
        account_set_id: AccountSetId,
        member_id: AccountSetMemberId,
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
    BalanceCreated {
        balance: BalanceSnapshot,
    },
    BalanceUpdated {
        balance: BalanceSnapshot,
    },
    EffectiveBalanceCreated {
        balance: EffectiveBalanceSnapshot,
    },
    EffectiveBalanceUpdated {
        balance: EffectiveBalanceSnapshot,
    },
}
