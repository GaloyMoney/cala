use es_entity::*;
use serde::{Deserialize, Serialize};

pub use cala_types::{
    account_set::*, primitives::AccountSetId, velocity::VelocityContextAccountValues,
};

use crate::primitives::*;

#[derive(EsEvent, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
#[serde(tag = "type", rename_all = "snake_case")]
#[es_event(id = "AccountSetId", event_context = false)]
pub enum AccountSetEvent {
    Initialized {
        values: AccountSetValues,
    },
    Updated {
        values: AccountSetValues,
        fields: Vec<String>,
    },
}

#[derive(EsEntity)]
pub struct AccountSet {
    pub id: AccountSetId,
    values: AccountSetValues,
    events: EntityEvents<AccountSetEvent>,
}

impl AccountSet {
    pub fn id(&self) -> AccountSetId {
        self.values.id
    }

    pub fn values(&self) -> &AccountSetValues {
        &self.values
    }

    pub fn update(&mut self, update: impl Into<AccountSetUpdate>) -> es_entity::Idempotent<()> {
        let AccountSetUpdate {
            external_id,
            name,
            normal_balance_type,
            description,
            metadata,
        } = update.into();
        let mut updated_fields = Vec::new();

        if let Some(name) = name {
            if name != self.values().name {
                self.values.name.clone_from(&name);
                updated_fields.push("name".to_string());
            }
        }
        if let Some(normal_balance_type) = normal_balance_type {
            if normal_balance_type != self.values().normal_balance_type {
                self.values
                    .normal_balance_type
                    .clone_from(&normal_balance_type);
                updated_fields.push("normal_balance_type".to_string());
            }
        }
        if external_id.is_some() && external_id != self.values().external_id {
            self.values.external_id.clone_from(&external_id);
            updated_fields.push("external_id".to_string());
        }
        if description.is_some() && description != self.values().description {
            self.values.description.clone_from(&description);
            updated_fields.push("description".to_string());
        }
        if let Some(metadata) = metadata {
            if metadata != serde_json::Value::Null
                && Some(&metadata) != self.values().metadata.as_ref()
            {
                self.values.metadata = Some(metadata);
                updated_fields.push("metadata".to_string());
            }
        }

        if updated_fields.is_empty() {
            return es_entity::Idempotent::AlreadyApplied;
        } else {
            self.events.push(AccountSetEvent::Updated {
                values: self.values.clone(),
                fields: updated_fields,
            });
        }
        es_entity::Idempotent::Executed(())
    }

    pub fn into_values(self) -> AccountSetValues {
        self.values
    }

    pub fn created_at(&self) -> chrono::DateTime<chrono::Utc> {
        self.events
            .entity_first_persisted_at()
            .expect("Entity not persisted")
    }

    pub fn modified_at(&self) -> chrono::DateTime<chrono::Utc> {
        self.events
            .entity_last_modified_at()
            .expect("Entity not persisted")
    }
}

#[derive(Debug, Default, bon::Builder)]
pub struct AccountSetUpdate {
    #[builder(into)]
    pub external_id: Option<String>,
    #[builder(into)]
    pub name: Option<String>,
    #[builder(into)]
    pub normal_balance_type: Option<DebitOrCredit>,
    #[builder(into)]
    pub description: Option<String>,
    #[builder(with = |metadata: impl serde::Serialize| -> Result<_, serde_json::Error> {
        serde_json::to_value(metadata)
    })]
    pub metadata: Option<serde_json::Value>,
}

impl From<(AccountSetValues, Vec<String>)> for AccountSetUpdate {
    fn from((values, fields): (AccountSetValues, Vec<String>)) -> Self {
        let mut update = AccountSetUpdate::default();

        for field in fields {
            match field.as_str() {
                "external_id" => {
                    if let Some(ref ext_id) = values.external_id {
                        update.external_id = Some(ext_id.clone());
                    }
                }
                "name" => {
                    update.name = Some(values.name.clone());
                }

                "normal_balance_type" => {
                    update.normal_balance_type = Some(values.normal_balance_type);
                }

                "description" => {
                    if let Some(ref desc) = values.description {
                        update.description = Some(desc.clone());
                    }
                }

                "metadata" => {
                    if let Some(metadata) = values.metadata.clone() {
                        update.metadata = Some(metadata);
                    }
                }
                _ => unreachable!("Unknown field: {}", field),
            }
        }
        update
    }
}

impl TryFromEvents<AccountSetEvent> for AccountSet {
    fn try_from_events(
        events: EntityEvents<AccountSetEvent>,
    ) -> Result<Self, EntityHydrationError> {
        let mut hydrator = AccountSet::hydrator();
        for event in events.iter_all() {
            match event {
                AccountSetEvent::Initialized { values } => {
                    hydrator.id(values.id).values(values.clone());
                }
                AccountSetEvent::Updated { values, .. } => {
                    hydrator.values(values.clone());
                }
            }
        }
        hydrator.build(events)
    }
}

/// Representation of a ***new*** ledger account set entity with required/optional properties and a builder.
#[derive(bon::Builder, Debug)]
pub struct NewAccountSet {
    #[builder(into)]
    pub id: AccountSetId,
    #[builder(into)]
    pub(super) name: String,
    #[builder(into)]
    pub(super) external_id: Option<String>,
    #[builder(into)]
    pub(super) journal_id: JournalId,
    #[builder(default)]
    pub(super) normal_balance_type: DebitOrCredit,
    pub(super) balance_rollup: BalanceRollup,
    #[builder(into)]
    pub(super) description: Option<String>,
    #[builder(with = |metadata: impl serde::Serialize| -> Result<_, serde_json::Error> {
        serde_json::to_value(metadata)
    })]
    pub(super) metadata: Option<serde_json::Value>,
}

impl NewAccountSet {
    pub(super) fn is_eventually_consistent(&self) -> bool {
        matches!(self.balance_rollup, BalanceRollup::EventuallyConsistent)
    }

    pub(super) fn context_values(&self) -> VelocityContextAccountValues {
        VelocityContextAccountValues {
            id: self.id.into(),
            name: self.name.clone(),
            normal_balance_type: self.normal_balance_type,
            external_id: self.external_id.clone(),
            metadata: self.metadata.clone(),
        }
    }
}

impl IntoEvents<AccountSetEvent> for NewAccountSet {
    type IdSource = FromCaller;

    fn into_events(
        self,
        builder: EntityEventsBuilder<AccountSetEvent, FromCaller>,
    ) -> EntityEvents<AccountSetEvent> {
        builder.initial_events(
            self.id,
            [AccountSetEvent::Initialized {
                values: AccountSetValues {
                    id: self.id,
                    version: 1,
                    journal_id: self.journal_id,
                    name: self.name,
                    external_id: self.external_id,
                    normal_balance_type: self.normal_balance_type,
                    description: self.description,
                    metadata: self.metadata,
                },
            }],
        )
    }
}
