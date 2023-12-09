use serde::{Deserialize, Serialize};

crate::entity_id! { AccountId }
crate::entity_id! { JournalId }
crate::entity_id! { TransactionId }
crate::entity_id! { EntryId }
crate::entity_id! { TxTemplateId }
crate::entity_id! { CorrelationId }

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq, sqlx::Type)]
#[sqlx(type_name = "DebitOrCredit", rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum DebitOrCredit {
    Debit,
    Credit,
}

impl Default for DebitOrCredit {
    fn default() -> Self {
        Self::Credit
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq, sqlx::Type)]
#[sqlx(type_name = "Status", rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Active,
    Locked,
}

impl Default for Status {
    fn default() -> Self {
        Self::Active
    }
}
