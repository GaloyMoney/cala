use cel_interpreter::{CelExpression, CelParseRejection};
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

#[derive(EsEntity)]
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
    fn try_from_events(
        events: EntityEvents<VelocityControlEvent>,
    ) -> Result<Self, EntityHydrationError> {
        let mut hydrator = VelocityControl::hydrator();
        for event in events.iter_all() {
            match event {
                VelocityControlEvent::Initialized { values } => {
                    hydrator.id(values.id).values(values.clone());
                }
            }
        }
        hydrator.build(events)
    }
}

/// Representation of a ***new*** velocity control entity with required/optional properties and a builder.
#[derive(Debug)]
pub struct NewVelocityControl {
    pub(super) id: VelocityControlId,
    pub(super) name: String,
    description: String,
    enforcement: NewVelocityEnforcement,
    condition: Option<String>,
}

#[bon::bon]
impl NewVelocityControl {
    #[builder]
    pub fn new(
        #[builder(into)] id: VelocityControlId,
        #[builder(into)] name: String,
        #[builder(into)] description: String,
        #[builder(into, default)] enforcement: NewVelocityEnforcement,
        #[builder(into)] condition: Option<String>,
    ) -> Result<Self, CelParseRejection> {
        validate_optional_expression(&condition)?;
        Ok(Self {
            id,
            name,
            description,
            enforcement,
            condition,
        })
    }
}

impl IntoEvents<VelocityControlEvent> for NewVelocityControl {
    type IdSource = FromCaller;

    fn into_events(
        self,
        builder: EntityEventsBuilder<VelocityControlEvent, FromCaller>,
    ) -> EntityEvents<VelocityControlEvent> {
        builder.initial_events(
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

fn validate_optional_expression(expr: &Option<String>) -> Result<(), CelParseRejection> {
    if let Some(expr) = expr {
        CelExpression::try_from(expr.as_str())?;
    }
    Ok(())
}

#[derive(bon::Builder, Debug, Clone, Default)]
pub struct NewVelocityEnforcement {
    #[builder(into, default)]
    pub(super) action: VelocityEnforcementAction,
}
