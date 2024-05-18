use super::{account::*, journal::*, primitives::*, transaction::*, tx_template::*};

pub(super) trait ToGlobalId {
    fn to_global_id(&self) -> async_graphql::types::ID;
}

impl From<cala_ledger::journal::JournalValues> for JournalCreatePayload {
    fn from(value: cala_ledger::journal::JournalValues) -> Self {
        JournalCreatePayload {
            journal: Journal::from(value),
        }
    }
}

impl From<cala_types::account::AccountValues> for AccountCreatePayload {
    fn from(value: cala_types::account::AccountValues) -> Self {
        Self {
            account: Account::from(value),
        }
    }
}

impl From<cala_types::tx_template::TxTemplateValues> for TxTemplateCreatePayload {
    fn from(value: cala_types::tx_template::TxTemplateValues) -> Self {
        Self {
            tx_template: TxTemplate::from(value),
        }
    }
}

impl From<cala_types::transaction::TransactionValues> for PostTransactionPayload {
    fn from(value: cala_types::transaction::TransactionValues) -> Self {
        Self {
            transaction: Transaction::from(value),
        }
    }
}

impl From<JSON> for cala_ledger::tx_template::TxParams {
    fn from(json: JSON) -> Self {
        let mut map = Self::default();

        let inner = json.into_inner();
        if let Some(object) = inner.as_object() {
            for (k, v) in object {
                map.insert(k.clone(), v.clone());
            }
        }
        map
    }
}
