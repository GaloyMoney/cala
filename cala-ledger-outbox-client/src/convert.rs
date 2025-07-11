use cala_types::{
    account::*, account_set::*, balance::*, entry::*, journal::*, outbox::*, primitives::*,
    transaction::*, tx_template::*,
};
use cel_interpreter::CelExpression;

use crate::{client::proto, error::*};

impl TryFrom<proto::CalaLedgerEvent> for OutboxEvent {
    type Error = CalaLedgerOutboxClientError;

    fn try_from(event: proto::CalaLedgerEvent) -> Result<Self, Self::Error> {
        let payload = OutboxEventPayload::try_from(
            event
                .payload
                .ok_or(CalaLedgerOutboxClientError::MissingField)?,
        )?;
        Ok(OutboxEvent {
            id: event.id.parse()?,
            sequence: EventSequence::from(event.sequence),
            payload,
            recorded_at: event
                .recorded_at
                .ok_or(CalaLedgerOutboxClientError::MissingField)?
                .into(),
        })
    }
}

impl TryFrom<proto::cala_ledger_event::Payload> for OutboxEventPayload {
    type Error = CalaLedgerOutboxClientError;

    fn try_from(payload: proto::cala_ledger_event::Payload) -> Result<Self, Self::Error> {
        use cala_types::outbox::OutboxEventPayload::*;
        let res = match payload {
            proto::cala_ledger_event::Payload::AccountCreated(proto::AccountCreated {
                data_source_id,
                account,
            }) => AccountCreated {
                source: data_source_id.parse()?,
                account: AccountValues::try_from(
                    account.ok_or(CalaLedgerOutboxClientError::MissingField)?,
                )?,
            },
            proto::cala_ledger_event::Payload::AccountUpdated(proto::AccountUpdated {
                data_source_id,
                account,
                fields,
            }) => AccountUpdated {
                source: data_source_id.parse()?,
                account: AccountValues::try_from(
                    account.ok_or(CalaLedgerOutboxClientError::MissingField)?,
                )?,
                fields,
            },
            proto::cala_ledger_event::Payload::AccountSetCreated(proto::AccountSetCreated {
                data_source_id,
                account_set,
            }) => AccountSetCreated {
                source: data_source_id.parse()?,
                account_set: AccountSetValues::try_from(
                    account_set.ok_or(CalaLedgerOutboxClientError::MissingField)?,
                )?,
            },
            proto::cala_ledger_event::Payload::AccountSetUpdated(proto::AccountSetUpdated {
                data_source_id,
                account_set,
                fields,
            }) => AccountSetUpdated {
                source: data_source_id.parse()?,
                account_set: AccountSetValues::try_from(
                    account_set.ok_or(CalaLedgerOutboxClientError::MissingField)?,
                )?,
                fields,
            },
            proto::cala_ledger_event::Payload::AccountSetMemberCreated(
                proto::AccountSetMemberCreated {
                    data_source_id,
                    member,
                },
            ) => {
                let member = member.ok_or(CalaLedgerOutboxClientError::MissingField)?;
                AccountSetMemberCreated {
                    source: data_source_id.parse()?,
                    account_set_id: member.account_set_id.parse()?,
                    member_id: match member
                        .member
                        .ok_or(CalaLedgerOutboxClientError::MissingField)?
                    {
                        proto::account_set_member::Member::MemberAccountId(account_id) => {
                            cala_types::account_set::AccountSetMemberId::from(
                                account_id.parse::<AccountId>()?,
                            )
                        }
                        proto::account_set_member::Member::MemberAccountSetId(account_set_id) => {
                            cala_types::account_set::AccountSetMemberId::from(
                                account_set_id.parse::<AccountSetId>()?,
                            )
                        }
                    },
                }
            }
            proto::cala_ledger_event::Payload::AccountSetMemberRemoved(
                proto::AccountSetMemberRemoved {
                    data_source_id,
                    member,
                },
            ) => {
                let member = member.ok_or(CalaLedgerOutboxClientError::MissingField)?;
                AccountSetMemberRemoved {
                    source: data_source_id.parse()?,
                    account_set_id: member.account_set_id.parse()?,
                    member_id: match member
                        .member
                        .ok_or(CalaLedgerOutboxClientError::MissingField)?
                    {
                        proto::account_set_member::Member::MemberAccountId(account_id) => {
                            cala_types::account_set::AccountSetMemberId::from(
                                account_id.parse::<AccountId>()?,
                            )
                        }
                        proto::account_set_member::Member::MemberAccountSetId(account_set_id) => {
                            cala_types::account_set::AccountSetMemberId::from(
                                account_set_id.parse::<AccountSetId>()?,
                            )
                        }
                    },
                }
            }
            proto::cala_ledger_event::Payload::JournalCreated(proto::JournalCreated {
                data_source_id,
                journal,
            }) => JournalCreated {
                source: data_source_id.parse()?,
                journal: JournalValues::try_from(
                    journal.ok_or(CalaLedgerOutboxClientError::MissingField)?,
                )?,
            },
            proto::cala_ledger_event::Payload::JournalUpdated(proto::JournalUpdated {
                data_source_id,
                journal,
                fields,
            }) => JournalUpdated {
                source: data_source_id.parse()?,
                journal: JournalValues::try_from(
                    journal.ok_or(CalaLedgerOutboxClientError::MissingField)?,
                )?,
                fields,
            },
            proto::cala_ledger_event::Payload::TxTemplateCreated(proto::TxTemplateCreated {
                data_source_id,
                tx_template,
            }) => TxTemplateCreated {
                source: data_source_id.parse()?,
                tx_template: TxTemplateValues::try_from(
                    tx_template.ok_or(CalaLedgerOutboxClientError::MissingField)?,
                )?,
            },
            proto::cala_ledger_event::Payload::TransactionCreated(proto::TransactionCreated {
                data_source_id,
                transaction,
            }) => TransactionCreated {
                source: data_source_id.parse()?,
                transaction: TransactionValues::try_from(
                    transaction.ok_or(CalaLedgerOutboxClientError::MissingField)?,
                )?,
            },
            proto::cala_ledger_event::Payload::TransactionUpdated(proto::TransactionUpdated {
                data_source_id,
                transaction,
                fields,
            }) => TransactionUpdated {
                source: data_source_id.parse()?,
                transaction: TransactionValues::try_from(
                    transaction.ok_or(CalaLedgerOutboxClientError::MissingField)?,
                )?,
                fields,
            },
            proto::cala_ledger_event::Payload::EntryCreated(proto::EntryCreated {
                data_source_id,
                entry,
            }) => EntryCreated {
                source: data_source_id.parse()?,
                entry: EntryValues::try_from(
                    entry.ok_or(CalaLedgerOutboxClientError::MissingField)?,
                )?,
            },
            proto::cala_ledger_event::Payload::BalanceCreated(proto::BalanceCreated {
                data_source_id,
                balance,
            }) => BalanceCreated {
                source: data_source_id.parse()?,
                balance: BalanceSnapshot::try_from(
                    balance.ok_or(CalaLedgerOutboxClientError::MissingField)?,
                )?,
            },
            proto::cala_ledger_event::Payload::BalanceUpdated(proto::BalanceUpdated {
                data_source_id,
                balance,
            }) => BalanceUpdated {
                source: data_source_id.parse()?,
                balance: BalanceSnapshot::try_from(
                    balance.ok_or(CalaLedgerOutboxClientError::MissingField)?,
                )?,
            },

            proto::cala_ledger_event::Payload::Empty(_) => Empty,
        };
        Ok(res)
    }
}

impl TryFrom<proto::Account> for AccountValues {
    type Error = CalaLedgerOutboxClientError;

    fn try_from(account: proto::Account) -> Result<Self, Self::Error> {
        let metadata = account.metadata.map(serde_json::to_value).transpose()?;
        let normal_balance_type =
            proto::DebitOrCredit::try_from(account.normal_balance_type).map(DebitOrCredit::from)?;
        let status = proto::Status::try_from(account.status).map(Status::from)?;
        let res = Self {
            id: account.id.parse()?,
            version: account.version,
            code: account.code,
            name: account.name,
            external_id: account.external_id,
            normal_balance_type,
            status,
            description: account.description,
            metadata,
            config: AccountConfig::from(
                account
                    .config
                    .ok_or(CalaLedgerOutboxClientError::MissingField)?,
            ),
        };
        Ok(res)
    }
}

impl From<proto::AccountConfig> for AccountConfig {
    fn from(config: proto::AccountConfig) -> Self {
        Self {
            is_account_set: config.is_account_set,
            eventually_consistent: config.eventually_consistent,
        }
    }
}

impl TryFrom<proto::AccountSet> for AccountSetValues {
    type Error = CalaLedgerOutboxClientError;

    fn try_from(account_set: proto::AccountSet) -> Result<Self, Self::Error> {
        let metadata = account_set.metadata.map(serde_json::to_value).transpose()?;
        let normal_balance_type = proto::DebitOrCredit::try_from(account_set.normal_balance_type)
            .map(DebitOrCredit::from)?;
        let res = Self {
            id: account_set.id.parse()?,
            version: account_set.version,
            journal_id: account_set.journal_id.parse()?,
            name: account_set.name,
            external_id: account_set.external_id,
            normal_balance_type,
            description: account_set.description,
            metadata,
        };
        Ok(res)
    }
}

impl TryFrom<proto::Journal> for JournalValues {
    type Error = CalaLedgerOutboxClientError;

    fn try_from(journal: proto::Journal) -> Result<Self, Self::Error> {
        let status = proto::Status::try_from(journal.status).map(Status::from)?;
        let res = Self {
            id: journal.id.parse()?,
            version: journal.version,
            name: journal.name,
            code: journal.code,
            status,
            description: journal.description,
            config: JournalConfig::from(
                journal
                    .config
                    .ok_or(CalaLedgerOutboxClientError::MissingField)?,
            ),
        };
        Ok(res)
    }
}

impl From<proto::JournalConfig> for JournalConfig {
    fn from(config: proto::JournalConfig) -> Self {
        Self {
            enable_effective_balances: config.enable_effective_balances,
        }
    }
}

impl From<proto::DebitOrCredit> for DebitOrCredit {
    fn from(dc: proto::DebitOrCredit) -> Self {
        match dc {
            proto::DebitOrCredit::Debit => DebitOrCredit::Debit,
            proto::DebitOrCredit::Credit => DebitOrCredit::Credit,
        }
    }
}

impl From<proto::Status> for Status {
    fn from(status: proto::Status) -> Self {
        match status {
            proto::Status::Active => Status::Active,
            proto::Status::Locked => Status::Locked,
        }
    }
}

impl TryFrom<proto::TxTemplate> for TxTemplateValues {
    type Error = CalaLedgerOutboxClientError;

    fn try_from(
        proto::TxTemplate {
            id,
            version,
            code,
            params,
            transaction,
            entries,
            description,
            metadata,
        }: proto::TxTemplate,
    ) -> Result<Self, Self::Error> {
        let params = params
            .into_iter()
            .map(ParamDefinition::try_from)
            .collect::<Result<Vec<_>, _>>()?;
        let transaction = TxTemplateTransaction::try_from(
            transaction.ok_or(CalaLedgerOutboxClientError::MissingField)?,
        )?;
        let entries = entries
            .into_iter()
            .map(TxTemplateEntry::try_from)
            .collect::<Result<Vec<_>, _>>()?;

        let res = Self {
            id: id.parse()?,
            version,
            code,
            params: Some(params),
            transaction,
            entries,
            description,
            metadata: metadata.map(serde_json::to_value).transpose()?,
        };
        Ok(res)
    }
}

impl TryFrom<proto::ParamDefinition> for ParamDefinition {
    type Error = CalaLedgerOutboxClientError;
    fn try_from(
        proto::ParamDefinition {
            name,
            data_type,
            default,
            description,
        }: proto::ParamDefinition,
    ) -> Result<Self, Self::Error> {
        let res = Self {
            name,
            r#type: proto::ParamDataType::try_from(data_type).map(ParamDataType::from)?,
            default: default.map(CelExpression::try_from).transpose()?,
            description,
        };
        Ok(res)
    }
}

impl TryFrom<proto::TxTemplateTransaction> for TxTemplateTransaction {
    type Error = CalaLedgerOutboxClientError;
    fn try_from(
        proto::TxTemplateTransaction {
            effective,
            journal_id,
            correlation_id,
            external_id,
            description,
            metadata,
        }: proto::TxTemplateTransaction,
    ) -> Result<Self, Self::Error> {
        let res = Self {
            effective: CelExpression::try_from(effective)?,
            journal_id: CelExpression::try_from(journal_id)?,
            correlation_id: correlation_id.map(CelExpression::try_from).transpose()?,
            external_id: external_id.map(CelExpression::try_from).transpose()?,
            description: description.map(CelExpression::try_from).transpose()?,
            metadata: metadata.map(CelExpression::try_from).transpose()?,
        };
        Ok(res)
    }
}

impl TryFrom<proto::TxTemplateEntry> for TxTemplateEntry {
    type Error = CalaLedgerOutboxClientError;
    fn try_from(
        proto::TxTemplateEntry {
            entry_type,
            account_id,
            layer,
            direction,
            units,
            currency,
            description,
            metadata,
        }: proto::TxTemplateEntry,
    ) -> Result<Self, Self::Error> {
        let res = Self {
            entry_type: CelExpression::try_from(entry_type)?,
            account_id: CelExpression::try_from(account_id)?,
            layer: CelExpression::try_from(layer)?,
            direction: CelExpression::try_from(direction)?,
            units: CelExpression::try_from(units)?,
            currency: CelExpression::try_from(currency)?,
            description: description.map(CelExpression::try_from).transpose()?,
            metadata: metadata.map(CelExpression::try_from).transpose()?,
        };
        Ok(res)
    }
}

impl From<proto::ParamDataType> for ParamDataType {
    fn from(data_type: proto::ParamDataType) -> Self {
        match data_type {
            proto::ParamDataType::String => ParamDataType::String,
            proto::ParamDataType::Integer => ParamDataType::Integer,
            proto::ParamDataType::Decimal => ParamDataType::Decimal,
            proto::ParamDataType::Boolean => ParamDataType::Boolean,
            proto::ParamDataType::Uuid => ParamDataType::Uuid,
            proto::ParamDataType::Date => ParamDataType::Date,
            proto::ParamDataType::Timestamp => ParamDataType::Timestamp,
            proto::ParamDataType::Json => ParamDataType::Json,
        }
    }
}

impl TryFrom<proto::Transaction> for TransactionValues {
    type Error = CalaLedgerOutboxClientError;
    fn try_from(
        proto::Transaction {
            id,
            version,
            created_at,
            modified_at,
            journal_id,
            tx_template_id,
            entry_ids,
            effective,
            correlation_id,
            external_id,
            description,
            metadata,
            void_of,
            voided_by,
        }: proto::Transaction,
    ) -> Result<Self, Self::Error> {
        let res = Self {
            id: id.parse()?,
            version,
            created_at: created_at
                .ok_or(CalaLedgerOutboxClientError::MissingField)?
                .into(),
            modified_at: modified_at
                .ok_or(CalaLedgerOutboxClientError::MissingField)?
                .into(),
            journal_id: journal_id.parse()?,
            tx_template_id: tx_template_id.parse()?,
            entry_ids: entry_ids
                .into_iter()
                .map(|id| id.parse())
                .collect::<Result<_, _>>()?,
            effective: effective.parse()?,
            voided_by: voided_by.map(|id| id.parse()).transpose()?,
            void_of: void_of.map(|id| id.parse()).transpose()?,
            correlation_id,
            external_id,
            description,
            metadata: metadata.map(serde_json::to_value).transpose()?,
        };
        Ok(res)
    }
}

impl TryFrom<proto::Entry> for EntryValues {
    type Error = CalaLedgerOutboxClientError;
    fn try_from(
        proto::Entry {
            id,
            version,
            journal_id,
            transaction_id,
            entry_type,
            sequence,
            account_id,
            layer,
            direction,
            units,
            currency,
            description,
            metadata,
        }: proto::Entry,
    ) -> Result<Self, Self::Error> {
        let res = Self {
            id: id.parse()?,
            version,
            journal_id: journal_id.parse()?,
            transaction_id: transaction_id.parse()?,
            account_id: account_id.parse()?,
            entry_type,
            sequence,
            layer: proto::Layer::try_from(layer).map(Layer::from)?,
            direction: proto::DebitOrCredit::try_from(direction).map(DebitOrCredit::from)?,
            units: units.parse()?,
            currency: currency.parse::<Currency>()?,
            description,
            metadata: metadata.map(serde_json::to_value).transpose()?,
        };
        Ok(res)
    }
}

impl TryFrom<proto::Balance> for BalanceSnapshot {
    type Error = CalaLedgerOutboxClientError;
    fn try_from(
        proto::Balance {
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
        }: proto::Balance,
    ) -> Result<Self, Self::Error> {
        let res = Self {
            journal_id: journal_id.parse()?,
            account_id: account_id.parse()?,
            currency: currency.parse()?,
            version,
            created_at: created_at
                .ok_or(CalaLedgerOutboxClientError::MissingField)?
                .into(),
            modified_at: modified_at
                .ok_or(CalaLedgerOutboxClientError::MissingField)?
                .into(),
            entry_id: entry_id.parse()?,
            settled: BalanceAmount::try_from(
                settled.ok_or(CalaLedgerOutboxClientError::MissingField)?,
            )?,
            pending: BalanceAmount::try_from(
                pending.ok_or(CalaLedgerOutboxClientError::MissingField)?,
            )?,
            encumbrance: BalanceAmount::try_from(
                encumbrance.ok_or(CalaLedgerOutboxClientError::MissingField)?,
            )?,
        };
        Ok(res)
    }
}

impl TryFrom<proto::BalanceAmount> for BalanceAmount {
    type Error = CalaLedgerOutboxClientError;

    fn try_from(amount: proto::BalanceAmount) -> Result<Self, Self::Error> {
        let res = Self {
            dr_balance: amount.dr_balance.parse()?,
            cr_balance: amount.cr_balance.parse()?,
            entry_id: amount.entry_id.parse()?,
            modified_at: amount
                .modified_at
                .ok_or(CalaLedgerOutboxClientError::MissingField)?
                .into(),
        };
        Ok(res)
    }
}

impl From<proto::Layer> for Layer {
    fn from(layer: proto::Layer) -> Self {
        match layer {
            proto::Layer::Settled => Layer::Settled,
            proto::Layer::Pending => Layer::Pending,
            proto::Layer::Encumbrance => Layer::Encumbrance,
        }
    }
}
