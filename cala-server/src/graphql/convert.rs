use super::{account::*, import_job::*, journal::*, primitives::*};

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

impl ToGlobalId for crate::primitives::ImportJobId {
    fn to_global_id(&self) -> async_graphql::types::ID {
        use base64::{engine::general_purpose, Engine as _};
        let id = format!(
            "import_job:{}",
            general_purpose::STANDARD_NO_PAD.encode(self.to_string())
        );
        async_graphql::types::ID::from(id)
    }
}

impl From<cala_ledger::Tag> for TAG {
    fn from(tag: cala_ledger::Tag) -> Self {
        TAG::from(tag.into_inner())
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
            tags: values.tags.into_iter().map(TAG::from).collect(),
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

impl From<cala_ledger::journal::JournalValues> for JournalCreatePayload {
    fn from(value: cala_ledger::journal::JournalValues) -> Self {
        JournalCreatePayload {
            journal: Journal::from(value),
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

impl From<ImportJobByNameCursor> for crate::import_job::ImportJobByNameCursor {
    fn from(cursor: ImportJobByNameCursor) -> Self {
        Self {
            name: cursor.name,
            id: cursor.id,
        }
    }
}

impl From<&crate::import_job::ImportJob> for ImportJobByNameCursor {
    fn from(job: &crate::import_job::ImportJob) -> Self {
        Self {
            name: job.name.clone(),
            id: job.id,
        }
    }
}

impl From<crate::import_job::ImportJob> for ImportJob {
    fn from(job: crate::import_job::ImportJob) -> Self {
        Self {
            id: job.id.to_global_id(),
            import_job_id: UUID::from(job.id),
            name: job.name,
            description: job.description,
        }
    }
}
