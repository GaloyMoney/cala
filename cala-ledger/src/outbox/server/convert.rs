use rust_decimal::prelude::ToPrimitive;

use cala_types::balance::BalanceSnapshot;

use crate::primitives::*;

use crate::{
    account::AccountValues,
    entry::*,
    journal::JournalValues,
    outbox::{
        error::OutboxError,
        event::{OutboxEvent, OutboxEventPayload},
    },
    transaction::TransactionValues,
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
            OutboxEventPayload::TransactionCreated {
                source,
                transaction,
            } => proto::cala_ledger_event::Payload::TransactionCreated(proto::TransactionCreated {
                data_source_id: source.to_string(),
                transaction: Some(proto::Transaction::from(transaction)),
            }),
            OutboxEventPayload::EntryCreated { source, entry } => {
                proto::cala_ledger_event::Payload::EntryCreated(proto::EntryCreated {
                    data_source_id: source.to_string(),
                    entry: Some(proto::Entry::from(entry)),
                })
            }
            OutboxEventPayload::BalanceCreated { source, balance } => {
                proto::cala_ledger_event::Payload::BalanceCreated(proto::BalanceCreated {
                    data_source_id: source.to_string(),
                    balance: Some(proto::Balance::from(balance)),
                })
            }
            OutboxEventPayload::BalanceUpdated { source, balance } => {
                proto::cala_ledger_event::Payload::BalanceUpdated(proto::BalanceUpdated {
                    data_source_id: source.to_string(),
                    balance: Some(proto::Balance::from(balance)),
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
            version,
            code,
            name,
            external_id,
            normal_balance_type,
            status,
            description,
            metadata,
        }: AccountValues,
    ) -> Self {
        let normal_balance_type: proto::DebitOrCredit = normal_balance_type.into();
        let status: proto::Status = status.into();
        proto::Account {
            id: id.to_string(),
            version,
            code,
            name,
            external_id,
            normal_balance_type: normal_balance_type as i32,
            status: status as i32,
            description,
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
            version,
            name,
            status,
            description,
        }: JournalValues,
    ) -> Self {
        let status: proto::Status = status.into();
        proto::Journal {
            id: id.to_string(),
            version,
            name,
            status: status as i32,
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
            version,
            code,
            params,
            tx_input,
            entries,
            description,
            metadata,
        }: TxTemplateValues,
    ) -> Self {
        let params = params
            .unwrap_or_default()
            .into_iter()
            .map(|param| param.into())
            .collect();
        proto::TxTemplate {
            id: id.to_string(),
            version,
            code,
            params,
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
            ParamDataType::String => proto::ParamDataType::String,
            ParamDataType::Integer => proto::ParamDataType::Integer,
            ParamDataType::Decimal => proto::ParamDataType::Decimal,
            ParamDataType::Boolean => proto::ParamDataType::Boolean,
            ParamDataType::Uuid => proto::ParamDataType::Uuid,
            ParamDataType::Date => proto::ParamDataType::Date,
            ParamDataType::Timestamp => proto::ParamDataType::Timestamp,
            ParamDataType::Json => proto::ParamDataType::Json,
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

impl From<TransactionValues> for proto::Transaction {
    fn from(
        TransactionValues {
            id,
            journal_id,
            tx_template_id,
            correlation_id,
            external_id,
            effective,
            description,
            metadata,
        }: TransactionValues,
    ) -> Self {
        proto::Transaction {
            id: id.to_string(),
            journal_id: journal_id.to_string(),
            tx_template_id: tx_template_id.to_string(),
            correlation_id,
            external_id,
            effective: effective.to_string(),
            description: description.map(String::from),
            metadata: metadata.map(|json| {
                serde_json::from_value(json).expect("Could not transfer json -> struct")
            }),
        }
    }
}

impl From<EntryValues> for proto::Entry {
    fn from(
        EntryValues {
            id,
            journal_id,
            transaction_id,
            account_id,
            entry_type,
            sequence,
            layer,
            direction,
            currency,
            units,
            description,
        }: EntryValues,
    ) -> Self {
        let layer: proto::Layer = layer.into();
        let direction: proto::DebitOrCredit = direction.into();
        let units = units.to_f64().expect("could not convert units to f64");
        proto::Entry {
            id: id.to_string(),
            journal_id: journal_id.to_string(),
            transaction_id: transaction_id.to_string(),
            account_id: account_id.to_string(),
            entry_type: entry_type.to_string(),
            sequence,
            layer: layer.into(),
            direction: direction.into(),
            currency: currency.to_string(),
            units: units.to_string(),
            description: description.map(String::from),
        }
    }
}

impl From<BalanceSnapshot> for proto::Balance {
    fn from(
        BalanceSnapshot {
            journal_id,
            account_id,
            currency,
            version,
            created_at,
            modified_at,
            entry_id,
            settled_dr_balance,
            settled_cr_balance,
            settled_entry_id,
            settled_modified_at,
            pending_dr_balance,
            pending_cr_balance,
            pending_entry_id,
            pending_modified_at,
            encumbered_dr_balance,
            encumbered_cr_balance,
            encumbered_entry_id,
            encumbered_modified_at,
        }: BalanceSnapshot,
    ) -> Self {
        proto::Balance {
            journal_id: journal_id.to_string(),
            account_id: account_id.to_string(),
            currency: currency.to_string(),
            version,
            created_at: Some(created_at.into()),
            modified_at: Some(modified_at.into()),
            entry_id: entry_id.to_string(),
            settled_dr_balance: settled_dr_balance.to_string(),
            settled_cr_balance: settled_cr_balance.to_string(),
            settled_entry_id: settled_entry_id.to_string(),
            settled_modified_at: Some(settled_modified_at.into()),
            pending_dr_balance: pending_dr_balance.to_string(),
            pending_cr_balance: pending_cr_balance.to_string(),
            pending_entry_id: pending_entry_id.to_string(),
            pending_modified_at: Some(pending_modified_at.into()),
            encumbered_dr_balance: encumbered_dr_balance.to_string(),
            encumbered_cr_balance: encumbered_cr_balance.to_string(),
            encumbered_entry_id: encumbered_entry_id.to_string(),
            encumbered_modified_at: Some(encumbered_modified_at.into()),
        }
    }
}

impl From<Layer> for proto::Layer {
    fn from(layer: Layer) -> Self {
        match layer {
            Layer::Settled => proto::Layer::Settled,
            Layer::Pending => proto::Layer::Pending,
            Layer::Encumbered => proto::Layer::Encumbered,
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
