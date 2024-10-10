use rust_decimal::prelude::ToPrimitive;

use cala_types::balance::{BalanceAmount, BalanceSnapshot};

use crate::primitives::*;

use crate::{
    account::*,
    account_set::*,
    entry::*,
    journal::JournalValues,
    outbox::event::{OutboxEvent, OutboxEventPayload},
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
            OutboxEventPayload::AccountUpdated {
                source,
                account,
                fields,
            } => proto::cala_ledger_event::Payload::AccountUpdated(proto::AccountUpdated {
                data_source_id: source.to_string(),
                account: Some(proto::Account::from(account)),
                fields,
            }),
            OutboxEventPayload::AccountSetCreated {
                source,
                account_set,
            } => proto::cala_ledger_event::Payload::AccountSetCreated(proto::AccountSetCreated {
                data_source_id: source.to_string(),
                account_set: Some(proto::AccountSet::from(account_set)),
            }),
            OutboxEventPayload::AccountSetUpdated {
                source,
                account_set,
                fields,
            } => proto::cala_ledger_event::Payload::AccountSetUpdated(proto::AccountSetUpdated {
                data_source_id: source.to_string(),
                account_set: Some(proto::AccountSet::from(account_set)),
                fields,
            }),
            OutboxEventPayload::AccountSetMemberCreated {
                source,
                account_set_id,
                member_id,
            } => proto::cala_ledger_event::Payload::AccountSetMemberCreated(
                proto::AccountSetMemberCreated {
                    data_source_id: source.to_string(),
                    member: Some(proto::AccountSetMember {
                        account_set_id: account_set_id.to_string(),
                        member: Some(match member_id {
                            AccountSetMemberId::Account(account_id) => {
                                proto::account_set_member::Member::MemberAccountId(
                                    account_id.to_string(),
                                )
                            }
                            AccountSetMemberId::AccountSet(account_set_id) => {
                                proto::account_set_member::Member::MemberAccountSetId(
                                    account_set_id.to_string(),
                                )
                            }
                        }),
                    }),
                },
            ),
            OutboxEventPayload::AccountSetMemberRemoved {
                source,
                account_set_id,
                member_id,
            } => proto::cala_ledger_event::Payload::AccountSetMemberRemoved(
                proto::AccountSetMemberRemoved {
                    data_source_id: source.to_string(),
                    member: Some(proto::AccountSetMember {
                        account_set_id: account_set_id.to_string(),
                        member: Some(match member_id {
                            AccountSetMemberId::Account(account_id) => {
                                proto::account_set_member::Member::MemberAccountId(
                                    account_id.to_string(),
                                )
                            }
                            AccountSetMemberId::AccountSet(account_set_id) => {
                                proto::account_set_member::Member::MemberAccountSetId(
                                    account_set_id.to_string(),
                                )
                            }
                        }),
                    }),
                },
            ),
            OutboxEventPayload::JournalCreated { source, journal } => {
                proto::cala_ledger_event::Payload::JournalCreated(proto::JournalCreated {
                    data_source_id: source.to_string(),
                    journal: Some(proto::Journal::from(journal)),
                })
            }
            OutboxEventPayload::JournalUpdated {
                source,
                journal,
                fields,
            } => proto::cala_ledger_event::Payload::JournalUpdated(proto::JournalUpdated {
                data_source_id: source.to_string(),
                journal: Some(proto::Journal::from(journal)),
                fields,
            }),
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
            created_at,
            modified_at,
            code,
            name,
            external_id,
            normal_balance_type,
            status,
            description,
            metadata,
            config,
        }: AccountValues,
    ) -> Self {
        let normal_balance_type: proto::DebitOrCredit = normal_balance_type.into();
        let status: proto::Status = status.into();
        proto::Account {
            id: id.to_string(),
            version,
            created_at: Some(created_at.into()),
            modified_at: Some(modified_at.into()),
            code,
            name,
            external_id,
            normal_balance_type: normal_balance_type as i32,
            status: status as i32,
            description,
            metadata: metadata.map(|json| {
                serde_json::from_value(json).expect("Could not transfer json -> struct")
            }),
            config: Some(proto::AccountConfig::from(config)),
        }
    }
}

impl From<AccountConfig> for proto::AccountConfig {
    fn from(config: AccountConfig) -> Self {
        proto::AccountConfig {
            is_account_set: config.is_account_set,
            eventually_consistent: config.eventually_consistent,
        }
    }
}

impl From<AccountSetValues> for proto::AccountSet {
    fn from(
        AccountSetValues {
            id,
            version,
            created_at,
            modified_at,
            journal_id,
            name,
            normal_balance_type,
            description,
            metadata,
        }: AccountSetValues,
    ) -> Self {
        let normal_balance_type: proto::DebitOrCredit = normal_balance_type.into();
        proto::AccountSet {
            id: id.to_string(),
            created_at: Some(created_at.into()),
            modified_at: Some(modified_at.into()),
            version,
            journal_id: journal_id.to_string(),
            name,
            normal_balance_type: normal_balance_type as i32,
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
            created_at,
            modified_at,
            name,
            status,
            description,
        }: JournalValues,
    ) -> Self {
        let status: proto::Status = status.into();
        proto::Journal {
            id: id.to_string(),
            version,
            created_at: Some(created_at.into()),
            modified_at: Some(modified_at.into()),
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
            created_at,
            modified_at,
            code,
            params,
            transaction,
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
            created_at: Some(created_at.into()),
            modified_at: Some(modified_at.into()),
            code,
            params,
            transaction: Some(transaction.into()),
            entries: entries.into_iter().map(|entry| entry.into()).collect(),
            description,
            metadata: metadata.map(|json| {
                serde_json::from_value(json).expect("Could not transfer json -> struct")
            }),
        }
    }
}

impl From<TxTemplateEntry> for proto::TxTemplateEntry {
    fn from(
        TxTemplateEntry {
            entry_type,
            account_id,
            layer,
            direction,
            currency,
            units,
            description,
        }: TxTemplateEntry,
    ) -> Self {
        proto::TxTemplateEntry {
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

impl From<TxTemplateTransaction> for proto::TxTemplateTransaction {
    fn from(
        TxTemplateTransaction {
            effective,
            journal_id,
            correlation_id,
            external_id,
            description,
            metadata,
        }: TxTemplateTransaction,
    ) -> Self {
        proto::TxTemplateTransaction {
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
            version,
            created_at,
            modified_at,
            journal_id,
            tx_template_id,
            correlation_id,
            external_id,
            effective,
            description,
            metadata,
            entry_ids,
        }: TransactionValues,
    ) -> Self {
        proto::Transaction {
            id: id.to_string(),
            version,
            created_at: Some(created_at.into()),
            modified_at: Some(modified_at.into()),
            journal_id: journal_id.to_string(),
            tx_template_id: tx_template_id.to_string(),
            entry_ids: entry_ids.into_iter().map(|id| id.to_string()).collect(),
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
            version,
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
            version,
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
            settled,
            pending,
            encumbrance,
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
            settled: Some(proto::BalanceAmount::from(settled)),
            pending: Some(proto::BalanceAmount::from(pending)),
            encumbrance: Some(proto::BalanceAmount::from(encumbrance)),
        }
    }
}

impl From<BalanceAmount> for proto::BalanceAmount {
    fn from(
        BalanceAmount {
            dr_balance,
            cr_balance,
            entry_id,
            modified_at,
        }: BalanceAmount,
    ) -> Self {
        proto::BalanceAmount {
            dr_balance: dr_balance.to_string(),
            cr_balance: cr_balance.to_string(),
            entry_id: entry_id.to_string(),
            modified_at: Some(modified_at.into()),
        }
    }
}

impl From<Layer> for proto::Layer {
    fn from(layer: Layer) -> Self {
        match layer {
            Layer::Settled => proto::Layer::Settled,
            Layer::Pending => proto::Layer::Pending,
            Layer::Encumbrance => proto::Layer::Encumbrance,
        }
    }
}
