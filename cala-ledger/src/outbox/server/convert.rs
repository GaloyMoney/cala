use crate::primitives::*;

use crate::{
    account::AccountValues,
    journal::JournalValues,
    outbox::{
        error::OutboxError,
        event::{OutboxEvent, OutboxEventPayload},
    },
};

use super::proto;

impl From<OutboxEvent> for proto::CalaLedgerEvent {
    fn from(
        OutboxEvent {
            id,
            sequence,
            payload,
            recorded_at,
        }: OutboxEvent,
    ) -> Self {
        let payload = match payload {
            OutboxEventPayload::AccountCreated { account } => {
                proto::cala_ledger_event::Payload::AccountCreated(proto::AccountCreated {
                    account: Some(proto::Account::from(account)),
                })
            }

            OutboxEventPayload::JournalCreated { journal } => {
                proto::cala_ledger_event::Payload::JournalCreated(proto::JournalCreated {
                    journal: Some(proto::Journal::from(journal)),
                })
            }
            OutboxEventPayload::Empty => proto::cala_ledger_event::Payload::Empty(true),
        };
        proto::CalaLedgerEvent {
            id: id.to_string(),
            sequence: u64::from(sequence),
            recorded_at: Some(recorded_at.into()),
            payload: Some(payload),
        }
    }
}

impl From<AccountValues> for proto::Account {
    fn from(
        AccountValues {
            id,
            code,
            name,
            external_id,
            normal_balance_type,
            status,
            description,
            tags,
            metadata,
            ..
        }: AccountValues,
    ) -> Self {
        let normal_balance_type: proto::DebitOrCredit = normal_balance_type.into();
        let status: proto::Status = status.into();
        proto::Account {
            id: id.to_string(),
            code,
            name,
            external_id,
            normal_balance_type: normal_balance_type as i32,
            status: status as i32,
            description,
            tags: tags.into_iter().map(|tag| tag.into_inner()).collect(),
            metadata: metadata.map(|json| {
                serde_json::from_value(json).expect("Could not transfer json -> struct")
            }),
        }
    }
}

impl From<JournalValues> for proto::Journal {
    fn from(
        JournalValues {
            id,
            name,
            status,
            external_id,
            description,
            ..
        }: JournalValues,
    ) -> Self {
        let status: proto::Status = status.into();
        proto::Journal {
            id: id.to_string(),
            name,
            status: status as i32,
            external_id,
            description,
        }
    }
}

impl From<DebitOrCredit> for proto::DebitOrCredit {
    fn from(priority: DebitOrCredit) -> Self {
        match priority {
            DebitOrCredit::Credit => proto::DebitOrCredit::Credit,
            DebitOrCredit::Debit => proto::DebitOrCredit::Debit,
        }
    }
}

impl From<Status> for proto::Status {
    fn from(priority: Status) -> Self {
        match priority {
            Status::Active => proto::Status::Active,
            Status::Locked => proto::Status::Locked,
        }
    }
}

impl From<OutboxError> for tonic::Status {
    fn from(err: OutboxError) -> Self {
        // match err {
        //     _ => tonic::Status::internal(err.to_string()),
        // }
        tonic::Status::internal(err.to_string())
    }
}
