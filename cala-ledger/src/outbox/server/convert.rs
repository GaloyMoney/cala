use crate::primitives::*;

use crate::{
    account::AccountValues,
    journal::JournalValues,
    outbox::{
        error::OutboxError,
        event::{OutboxEvent, OutboxEventPayload},
    },
    tx_template::*,
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
            OutboxEventPayload::AccountCreated { source, account } => {
                proto::cala_ledger_event::Payload::AccountCreated(proto::AccountCreated {
                    data_source_id: source.to_string(),
                    account: Some(proto::Account::from(account)),
                })
            }
            OutboxEventPayload::JournalCreated { source, journal } => {
                proto::cala_ledger_event::Payload::JournalCreated(proto::JournalCreated {
                    data_source_id: source.to_string(),
                    journal: Some(proto::Journal::from(journal)),
                })
            }
            OutboxEventPayload::TxTemplateCreated {
                source,
                tx_template,
            } => proto::cala_ledger_event::Payload::TxTemplateCreated(proto::TxTemplateCreated {
                data_source_id: source.to_string(),
                tx_template: Some(proto::TxTemplate::from(tx_template)),
            }),
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

impl From<TxTemplateValues> for proto::TxTemplate {
    fn from(
        TxTemplateValues {
            id,
            code,
            params,
            tx_input,
            entries,
            description,
            metadata,
        }: TxTemplateValues,
    ) -> Self {
        proto::TxTemplate {
            id: id.to_string(),
            code,
            params: params.into_iter().map(|param| param.into()).collect(),
            tx_input: Some(tx_input.into()),
            entries: entries.into_iter().map(|entry| entry.into()).collect(),
            description,
            metadata: metadata.map(|json| {
                serde_json::from_value(json).expect("Could not transfer json -> struct")
            }),
        }
    }
}

impl From<EntryInput> for proto::EntryInput {
    fn from(
        EntryInput {
            entry_type,
            account_id,
            layer,
            direction,
            currency,
            units,
            description,
        }: EntryInput,
    ) -> Self {
        proto::EntryInput {
            entry_type: String::from(entry_type),
            account_id: String::from(account_id),
            layer: String::from(layer),
            direction: String::from(direction),
            currency: String::from(currency),
            units: String::from(units),
            description: description.map(String::from),
        }
    }
}

impl From<ParamDefinition> for proto::ParamDefinition {
    fn from(
        ParamDefinition {
            name,
            r#type,
            default,
            description,
        }: ParamDefinition,
    ) -> Self {
        let data_type: proto::ParamDataType = r#type.into();
        proto::ParamDefinition {
            name,
            data_type: data_type as i32,
            default: default.map(String::from),
            description,
        }
    }
}

impl From<ParamDataType> for proto::ParamDataType {
    fn from(param_data_type: ParamDataType) -> Self {
        match param_data_type {
            ParamDataType::STRING => proto::ParamDataType::String,
            ParamDataType::INTEGER => proto::ParamDataType::Integer,
            ParamDataType::DECIMAL => proto::ParamDataType::Decimal,
            ParamDataType::BOOLEAN => proto::ParamDataType::Boolean,
            ParamDataType::UUID => proto::ParamDataType::Uuid,
            ParamDataType::DATE => proto::ParamDataType::Date,
            ParamDataType::TIMESTAMP => proto::ParamDataType::Timestamp,
            ParamDataType::JSON => proto::ParamDataType::Json,
        }
    }
}

impl From<TxInput> for proto::TxInput {
    fn from(
        TxInput {
            effective,
            journal_id,
            correlation_id,
            external_id,
            description,
            metadata,
        }: TxInput,
    ) -> Self {
        proto::TxInput {
            effective: String::from(effective),
            journal_id: String::from(journal_id),
            correlation_id: correlation_id.map(String::from),
            external_id: external_id.map(String::from),
            description: description.map(String::from),
            metadata: metadata.map(String::from),
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
