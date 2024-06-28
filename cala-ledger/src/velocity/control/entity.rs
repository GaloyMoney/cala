use cel_interpreter::CelExpression;
use derive_builder::Builder;
use serde::{Deserialize, Serialize};

pub use crate::{entity::*, param::definition::*};
pub use cala_types::{
    primitives::{Currency, VelocityControlId},
    velocity::*,
};

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum VelocityControlEvent {
    Initialized { values: VelocityControlValues },
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
    _values: VelocityControlValues,
    pub(super) _events: EntityEvents<VelocityControlEvent>,
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
                    builder = builder._values(values.clone());
                }
            }
        }
        builder._events(events).build()
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
    enforcement: VelocityEnforcement,
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
                    enforcement: self.enforcement,
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
