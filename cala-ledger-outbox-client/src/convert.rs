use cala_types::{account::*, outbox::*, primitives::*};

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
                account,
            }) => AccountCreated {
                account: AccountValues::try_from(
                    account.ok_or(CalaLedgerOutboxClientError::MissingField)?,
                )?,
            },
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
            code: account.code,
            name: account.name,
            external_id: account.external_id,
            normal_balance_type,
            status,
            description: account.description,
            tags: account.tags,
            metadata,
        };
        Ok(res)
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
