use cel_interpreter::CelExpression;
use derive_builder::Builder;
use serde::{Deserialize, Serialize};

pub use crate::{entity::*, param::definition::*};
pub use cala_types::{
    primitives::{Currency, VelocityControlId, VelocityLimitId},
    velocity::*,
};

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum VelocityControlEvent {
    Initialized { values: VelocityControlValues },
    AddLimit { limit_id: VelocityLimitId },
}

impl EntityEvent for VelocityControlEvent {
    type EntityId = VelocityControlId;
    fn event_table_name() -> &'static str {
        "cala_velocity_control_events"
    }
}

#[derive(Builder)]
#[builder(pattern = "owned", build_fn(error = "EntityError"))]
pub struct VelocityControl {
    values: VelocityControlValues,
    pub(super) events: EntityEvents<VelocityControlEvent>,
}

impl VelocityControl {
    pub fn id(&self) -> VelocityControlId {
        self.values.id
    }

    pub fn into_values(self) -> VelocityControlValues {
        self.values
    }

    pub fn created_at(&self) -> chrono::DateTime<chrono::Utc> {
        self.events
            .entity_first_persisted_at
            .expect("No events for account")
    }

    pub fn add_limit(&mut self, limit_id: VelocityLimitId) {
        self.events
            .push(VelocityControlEvent::AddLimit { limit_id });
    }
}

impl Entity for VelocityControl {
    type Event = VelocityControlEvent;
}

impl TryFrom<EntityEvents<VelocityControlEvent>> for VelocityControl {
    type Error = EntityError;

    fn try_from(events: EntityEvents<VelocityControlEvent>) -> Result<Self, Self::Error> {
        let mut builder = VelocityControlBuilder::default();
        for event in events.iter() {
            match event {
                VelocityControlEvent::Initialized { values } => {
                    builder = builder.values(values.clone());
                }
                _ => {}
            }
        }
        builder.events(events).build()
    }
}

/// Representation of a ***new*** velocity control entity with required/optional properties and a builder.
#[derive(Builder, Debug)]
#[builder(build_fn(validate = "Self::validate"))]
pub struct NewVelocityControl {
    #[builder(setter(into))]
    pub(super) id: VelocityControlId,
    #[builder(setter(into))]
    pub(super) name: String,
    #[builder(setter(into))]
    description: String,
    #[builder(setter(into), default)]
    enforcement: NewVelocityEnforcement,
    #[builder(setter(strip_option, into), default)]
    condition: Option<String>,
}

impl NewVelocityControl {
    pub fn builder() -> NewVelocityControlBuilder {
        NewVelocityControlBuilder::default()
    }

    pub(super) fn initial_events(self) -> EntityEvents<VelocityControlEvent> {
        EntityEvents::init(
            self.id,
            [VelocityControlEvent::Initialized {
                values: VelocityControlValues {
                    id: self.id,
                    name: self.name,
                    description: self.description,
                    enforcement: self.enforcement.action.into(),
                    condition: self
                        .condition
                        .map(|expr| CelExpression::try_from(expr).expect("already validated")),
                },
            }],
        )
    }
}

impl NewVelocityControlBuilder {
    fn validate(&self) -> Result<(), String> {
        validate_optional_expression(&self.condition)?;
        Ok(())
    }
}

fn validate_optional_expression(expr: &Option<Option<String>>) -> Result<(), String> {
    if let Some(Some(expr)) = expr.as_ref() {
        CelExpression::try_from(expr.as_str()).map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[derive(Builder, Debug, Clone, Default)]
pub struct NewVelocityEnforcement {
    #[builder(setter(into), default)]
    pub(super) action: VelocityEnforcementAction,
}

impl NewVelocityEnforcement {
    pub fn builder() -> NewVelocityEnforcementBuilder {
        NewVelocityEnforcementBuilder::default()
    }
}
