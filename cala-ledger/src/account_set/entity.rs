use derive_builder::Builder;
use serde::{Deserialize, Serialize};

pub use cala_types::{account_set::*, primitives::AccountSetId};

use crate::{entity::*, primitives::*};

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AccountSetEvent {
    #[cfg(feature = "import")]
    Imported {
        source: DataSource,
        values: AccountSetValues,
    },
    Initialized {
        values: AccountSetValues,
    },
}

impl EntityEvent for AccountSetEvent {
    type EntityId = AccountSetId;
    fn event_table_name() -> &'static str {
        "cala_account_events"
    }
}

#[derive(Builder)]
#[builder(pattern = "owned", build_fn(error = "EntityError"))]
pub struct AccountSet {
    values: AccountSetValues,
    pub(super) events: EntityEvents<AccountSetEvent>,
}

impl Entity for AccountSet {
    type Event = AccountSetEvent;
}

impl AccountSet {
    #[cfg(feature = "import")]
    pub(super) fn import(source: DataSourceId, values: AccountSetValues) -> Self {
        let events = EntityEvents::init(
            values.id,
            [AccountSetEvent::Imported {
                source: DataSource::Remote { id: source },
                values,
            }],
        );
        Self::try_from(events).expect("Failed to build account from events")
    }

    pub fn id(&self) -> AccountSetId {
        self.values.id
    }

    pub fn values(&self) -> &AccountSetValues {
        &self.values
    }

    pub fn into_values(self) -> AccountSetValues {
        self.values
    }

    pub fn created_at(&self) -> chrono::DateTime<chrono::Utc> {
        self.events
            .entity_first_persisted_at
            .expect("No events for account")
    }

    pub fn modified_at(&self) -> chrono::DateTime<chrono::Utc> {
        self.events
            .latest_event_persisted_at
            .expect("No events for account")
    }
}

impl TryFrom<EntityEvents<AccountSetEvent>> for AccountSet {
    type Error = EntityError;

    fn try_from(events: EntityEvents<AccountSetEvent>) -> Result<Self, Self::Error> {
        let mut builder = AccountSetBuilder::default();
        for event in events.iter() {
            match event {
                #[cfg(feature = "import")]
                AccountSetEvent::Imported { source: _, values } => {
                    builder = builder.values(values.clone());
                }
                AccountSetEvent::Initialized { values } => {
                    builder = builder.values(values.clone());
                }
            }
        }
        builder.events(events).build()
    }
}

/// Representation of a ***new*** ledger account entity with required/optional properties and a builder.
#[derive(Builder, Debug)]
pub struct NewAccountSet {
    #[builder(setter(into))]
    pub id: AccountSetId,
    #[builder(setter(into))]
    pub(super) name: String,
    #[builder(setter(into))]
    pub(super) journal_id: JournalId,
    #[builder(default)]
    pub(super) normal_balance_type: DebitOrCredit,
    #[builder(setter(strip_option, into), default)]
    pub(super) description: Option<String>,
    #[builder(setter(custom), default)]
    pub(super) metadata: Option<serde_json::Value>,
}

impl NewAccountSet {
    pub fn builder() -> NewAccountSetBuilder {
        NewAccountSetBuilder::default()
    }

    pub(super) fn initial_events(self) -> EntityEvents<AccountSetEvent> {
        EntityEvents::init(
            self.id,
            [AccountSetEvent::Initialized {
                values: AccountSetValues {
                    id: self.id,
                    version: 1,
                    journal_id: self.journal_id,
                    name: self.name,
                    normal_balance_type: self.normal_balance_type,
                    description: self.description,
                    metadata: self.metadata,
                },
            }],
        )
    }
}

impl NewAccountSetBuilder {
    pub fn metadata<T: serde::Serialize>(
        &mut self,
        metadata: T,
    ) -> Result<&mut Self, serde_json::Error> {
        self.metadata = Some(Some(serde_json::to_value(metadata)?));
        Ok(self)
    }
}
