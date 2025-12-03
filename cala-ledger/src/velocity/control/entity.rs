use cel_interpreter::CelExpression;
use derive_builder::Builder;
use es_entity::*;
use serde::{Deserialize, Serialize};

pub use crate::param::definition::*;
pub use cala_types::{primitives::*, velocity::*};

#[derive(EsEvent, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
#[serde(tag = "type", rename_all = "snake_case")]
#[es_event(id = "VelocityControlId", event_context = false)]
pub enum VelocityControlEvent {
    Initialized { values: VelocityControlValues },
}

#[derive(EsEntity, Builder)]
#[builder(pattern = "owned", build_fn(error = "EsEntityError"))]
pub struct VelocityControl {
    pub id: VelocityControlId,
    values: VelocityControlValues,
    events: EntityEvents<VelocityControlEvent>,
}

impl VelocityControl {
    pub fn id(&self) -> VelocityControlId {
        self.values.id
    }

    pub fn into_values(self) -> VelocityControlValues {
        self.values
    }

    pub fn values(&self) -> &VelocityControlValues {
        &self.values
    }

    pub fn created_at(&self) -> chrono::DateTime<chrono::Utc> {
        self.events
            .entity_first_persisted_at()
            .expect("Entity not persisted")
    }
}

impl TryFromEvents<VelocityControlEvent> for VelocityControl {
    fn try_from_events(events: EntityEvents<VelocityControlEvent>) -> Result<Self, EsEntityError> {
        let mut builder = VelocityControlBuilder::default();
        for event in events.iter_all() {
            match event {
                VelocityControlEvent::Initialized { values } => {
                    builder = builder.id(values.id).values(values.clone());
                }
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

    pub(super) fn data_source(&self) -> DataSource {
        DataSource::Local
    }
}

impl IntoEvents<VelocityControlEvent> for NewVelocityControl {
    fn into_events(self) -> EntityEvents<VelocityControlEvent> {
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
