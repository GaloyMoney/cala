use super::{account::*, job::*, journal::*, primitives::*, tx_template::*};

trait ToGlobalId {
    fn to_global_id(&self) -> async_graphql::types::ID;
}

impl From<AccountByNameCursor> for cala_ledger::account::AccountByNameCursor {
    fn from(cursor: AccountByNameCursor) -> Self {
        Self {
            name: cursor.name,
            id: cursor.id,
        }
    }
}

impl ToGlobalId for cala_ledger::AccountId {
    fn to_global_id(&self) -> async_graphql::types::ID {
        use base64::{engine::general_purpose, Engine as _};
        let id = format!(
            "account:{}",
            general_purpose::STANDARD_NO_PAD.encode(self.to_string())
        );
        async_graphql::types::ID::from(id)
    }
}

impl ToGlobalId for cala_ledger::JournalId {
    fn to_global_id(&self) -> async_graphql::types::ID {
        use base64::{engine::general_purpose, Engine as _};
        let id = format!(
            "journal:{}",
            general_purpose::STANDARD_NO_PAD.encode(self.to_string())
        );
        async_graphql::types::ID::from(id)
    }
}

impl ToGlobalId for cala_ledger::TxTemplateId {
    fn to_global_id(&self) -> async_graphql::types::ID {
        use base64::{engine::general_purpose, Engine as _};
        let id = format!(
            "tx_template:{}",
            general_purpose::STANDARD_NO_PAD.encode(self.to_string())
        );
        async_graphql::types::ID::from(id)
    }
}

impl ToGlobalId for crate::primitives::JobId {
    fn to_global_id(&self) -> async_graphql::types::ID {
        use base64::{engine::general_purpose, Engine as _};
        let id = format!(
            "job:{}",
            general_purpose::STANDARD_NO_PAD.encode(self.to_string())
        );
        async_graphql::types::ID::from(id)
    }
}

impl From<cala_ledger::account::AccountValues> for Account {
    fn from(values: cala_ledger::account::AccountValues) -> Self {
        Self {
            id: values.id.to_global_id(),
            account_id: UUID::from(values.id),
            code: values.code,
            name: values.name,
            normal_balance_type: DebitOrCredit::from(values.normal_balance_type),
            status: Status::from(values.status),
            external_id: values.external_id,
            description: values.description,
            metadata: values.metadata.map(JSON::from),
        }
    }
}

impl From<cala_ledger::journal::JournalValues> for Journal {
    fn from(value: cala_ledger::journal::JournalValues) -> Self {
        Self {
            id: value.id.to_global_id(),
            journal_id: UUID::from(value.id),
            name: value.name,
            external_id: value.external_id,
            status: Status::from(value.status),
            description: value.description,
        }
    }
}

impl From<cala_ledger::tx_template::TxTemplateValues> for TxTemplate {
    fn from(value: cala_ledger::tx_template::TxTemplateValues) -> Self {
        let tx_input = TxInput::from(value.tx_input);
        let entries = value.entries.into_iter().map(EntryInput::from).collect();
        let params = value
            .params
            .map(|params| params.into_iter().map(ParamDefinition::from).collect());
        Self {
            id: value.id.to_global_id(),
            tx_template_id: UUID::from(value.id),
            code: value.code,
            tx_input,
            entries,
            params,
            description: value.description,
            metadata: value.metadata.map(JSON::from),
        }
    }
}

impl From<cala_ledger::tx_template::TxInput> for TxInput {
    fn from(
        cala_ledger::tx_template::TxInput {
            effective,
            journal_id,
            correlation_id,
            external_id,
            description,
            metadata,
        }: cala_ledger::tx_template::TxInput,
    ) -> Self {
        Self {
            effective: Expression::from(effective),
            journal_id: Expression::from(journal_id),
            correlation_id: correlation_id.map(Expression::from),
            external_id: external_id.map(Expression::from),
            description: description.map(Expression::from),
            metadata: metadata.map(Expression::from),
        }
    }
}

impl From<cala_ledger::tx_template::EntryInput> for EntryInput {
    fn from(
        cala_ledger::tx_template::EntryInput {
            entry_type,
            account_id,
            layer,
            direction,
            units,
            currency,
            description,
        }: cala_ledger::tx_template::EntryInput,
    ) -> Self {
        Self {
            entry_type: Expression::from(entry_type),
            account_id: Expression::from(account_id),
            layer: Expression::from(layer),
            direction: Expression::from(direction),
            units: Expression::from(units),
            currency: Expression::from(currency),
            description: description.map(Expression::from),
        }
    }
}

impl From<cala_ledger::tx_template::ParamDefinition> for ParamDefinition {
    fn from(value: cala_ledger::tx_template::ParamDefinition) -> Self {
        let default = value.default.map(Expression::from);
        Self {
            name: value.name,
            r#type: ParamDataType::from(value.r#type),
            default,
            description: value.description,
        }
    }
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

impl From<&cala_ledger::account::AccountValues> for AccountByNameCursor {
    fn from(values: &cala_ledger::account::AccountValues) -> Self {
        Self {
            name: values.name.clone(),
            id: values.id,
        }
    }
}

impl From<JobByNameCursor> for crate::job::JobByNameCursor {
    fn from(cursor: JobByNameCursor) -> Self {
        Self {
            name: cursor.name,
            id: cursor.id,
        }
    }
}

impl From<&crate::job::Job> for JobByNameCursor {
    fn from(job: &crate::job::Job) -> Self {
        Self {
            name: job.name.clone(),
            id: job.id,
        }
    }
}

impl From<crate::job::Job> for Job {
    fn from(job: crate::job::Job) -> Self {
        Self {
            id: job.id.to_global_id(),
            job_id: UUID::from(job.id),
            name: job.name,
            description: job.description,
        }
    }
}
