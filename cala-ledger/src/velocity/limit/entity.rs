use cel_interpreter::CelExpression;
use derive_builder::Builder;
use serde::{Deserialize, Serialize};

pub use crate::{entity::*, param::definition::*};
pub use cala_types::{
    primitives::{Currency, VelocityLimitId},
    velocity::*,
};

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum VelocityLimitEvent {
    Initialized { values: VelocityLimitValues },
}

impl EntityEvent for VelocityLimitEvent {
    type EntityId = VelocityLimitId;
    fn event_table_name() -> &'static str {
        "cala_velocity_limit_events"
    }
}

#[derive(Builder)]
#[builder(pattern = "owned", build_fn(error = "EntityError"))]
pub struct VelocityLimit {
    _values: VelocityLimitValues,
    pub(super) _events: EntityEvents<VelocityLimitEvent>,
}

impl Entity for VelocityLimit {
    type Event = VelocityLimitEvent;
}

impl TryFrom<EntityEvents<VelocityLimitEvent>> for VelocityLimit {
    type Error = EntityError;

    fn try_from(events: EntityEvents<VelocityLimitEvent>) -> Result<Self, Self::Error> {
        let mut builder = VelocityLimitBuilder::default();
        for event in events.iter() {
            match event {
                VelocityLimitEvent::Initialized { values } => {
                    builder = builder._values(values.clone());
                }
            }
        }
        builder._events(events).build()
    }
}

/// Representation of a ***new*** velocity limit entity with required/optional properties and a builder.
#[derive(Builder, Debug)]
#[builder(build_fn(validate = "Self::validate"))]
pub struct NewVelocityLimit {
    #[builder(setter(into))]
    pub(super) id: VelocityLimitId,
    #[builder(setter(into))]
    pub(super) name: String,
    #[builder(setter(into))]
    description: String,
    window: Vec<NewPartitionKeyInput>,
    #[builder(setter(strip_option, into), default)]
    condition: Option<String>,
    currency: Option<Currency>,
    #[builder(setter(strip_option), default)]
    params: Option<Vec<NewParamDefinition>>,
    limit: NewLimitInput,
}

impl NewVelocityLimit {
    pub fn builder() -> NewVelocityLimitBuilder {
        NewVelocityLimitBuilder::default()
    }

    pub(super) fn initial_events(self) -> EntityEvents<VelocityLimitEvent> {
        let limit = self.limit;
        EntityEvents::init(
            self.id,
            [VelocityLimitEvent::Initialized {
                values: VelocityLimitValues {
                    id: self.id,
                    name: self.name,
                    description: self.description,
                    currency: self.currency,
                    window: self
                        .window
                        .into_iter()
                        .map(|input| PartitionKeyInput {
                            alias: input.alias,
                            value: CelExpression::try_from(input.value).expect("already validated"),
                        })
                        .collect(),
                    condition: self
                        .condition
                        .map(|expr| CelExpression::try_from(expr).expect("already validated")),
                    params: self
                        .params
                        .map(|params| params.into_iter().map(ParamDefinition::from).collect()),
                    limit: LimitInput {
                        timestamp_source: limit
                            .timestamp_source
                            .map(CelExpression::try_from)
                            .transpose()
                            .expect("already validated"),
                        balance: limit
                            .balance
                            .into_iter()
                            .map(|input| BalanceLimitInput {
                                layer: CelExpression::try_from(input.layer)
                                    .expect("already validated"),
                                amount: CelExpression::try_from(input.amount)
                                    .expect("already validated"),
                                enforcement_direction: CelExpression::try_from(
                                    input.enforcement_direction,
                                )
                                .expect("already validated"),
                            })
                            .collect(),
                    },
                },
            }],
        )
    }
}

impl NewVelocityLimitBuilder {
    fn validate(&self) -> Result<(), String> {
        validate_optional_expression(&self.condition)?;
        Ok(())
    }
}

#[derive(Clone, Builder, Debug)]
#[builder(build_fn(validate = "Self::validate"))]
pub struct NewPartitionKeyInput {
    #[builder(setter(into))]
    alias: String,
    #[builder(setter(into))]
    value: String,
}
impl NewPartitionKeyInput {
    pub fn builder() -> NewPartitionKeyInputBuilder {
        NewPartitionKeyInputBuilder::default()
    }
}
impl NewPartitionKeyInputBuilder {
    fn validate(&self) -> Result<(), String> {
        validate_expression(
            self.value
                .as_ref()
                .expect("Mandatory field 'value' not set"),
        )?;
        Ok(())
    }
}

#[derive(Clone, Builder, Debug)]
#[builder(build_fn(validate = "Self::validate"))]
pub struct NewLimitInput {
    #[builder(setter(strip_option, into), default)]
    timestamp_source: Option<String>,
    balance: Vec<NewBalanceLimitInput>,
}
impl NewLimitInput {
    pub fn builder() -> NewLimitInputBuilder {
        NewLimitInputBuilder::default()
    }
}
impl NewLimitInputBuilder {
    fn validate(&self) -> Result<(), String> {
        validate_optional_expression(&self.timestamp_source)
    }
}

#[derive(Clone, Builder, Debug)]
#[builder(build_fn(validate = "Self::validate"))]
pub struct NewBalanceLimitInput {
    #[builder(setter(into))]
    layer: String,
    #[builder(setter(into))]
    amount: String,
    #[builder(setter(into))]
    enforcement_direction: String,
}
impl NewBalanceLimitInput {
    pub fn builder() -> NewBalanceLimitInputBuilder {
        NewBalanceLimitInputBuilder::default()
    }
}
impl NewBalanceLimitInputBuilder {
    fn validate(&self) -> Result<(), String> {
        validate_expression(
            self.layer
                .as_ref()
                .expect("Mandatory field 'value' not set"),
        )?;
        validate_expression(
            self.amount
                .as_ref()
                .expect("Mandatory field 'value' not set"),
        )?;
        validate_expression(
            self.enforcement_direction
                .as_ref()
                .expect("Mandatory field 'value' not set"),
        )?;
        Ok(())
    }
}

fn validate_expression(expr: &str) -> Result<(), String> {
    CelExpression::try_from(expr).map_err(|e| e.to_string())?;
    Ok(())
}
fn validate_optional_expression(expr: &Option<Option<String>>) -> Result<(), String> {
    if let Some(Some(expr)) = expr.as_ref() {
        CelExpression::try_from(expr.as_str()).map_err(|e| e.to_string())?;
    }
    Ok(())
}
