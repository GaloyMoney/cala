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
    values: VelocityLimitValues,
    pub(super) _events: EntityEvents<VelocityLimitEvent>,
}

impl VelocityLimit {
    pub fn id(&self) -> VelocityLimitId {
        self.values.id
    }

    pub fn into_values(self) -> VelocityLimitValues {
        self.values
    }
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
                    builder = builder.values(values.clone());
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
    window: Vec<NewPartitionKey>,
    #[builder(setter(strip_option, into), default)]
    condition: Option<String>,
    currency: Option<Currency>,
    #[builder(setter(strip_option), default)]
    params: Option<Vec<NewParamDefinition>>,
    limit: NewLimit,
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
                        .map(|input| PartitionKey {
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
                    limit: Limit {
                        timestamp_source: limit
                            .timestamp_source
                            .map(CelExpression::try_from)
                            .transpose()
                            .expect("already validated"),
                        balance: limit
                            .balance
                            .into_iter()
                            .map(|input| BalanceLimit {
                                limit_type: input.limit_type,
                                layer: CelExpression::try_from(input.layer)
                                    .expect("already validated"),
                                amount: CelExpression::try_from(input.amount)
                                    .expect("already validated"),
                                enforcement_direction: CelExpression::try_from(
                                    input.enforcement_direction,
                                )
                                .expect("already validated"),
                                start: input.start.map(|expr| {
                                    CelExpression::try_from(expr).expect("already validated")
                                }),
                                end: input.end.map(|expr| {
                                    CelExpression::try_from(expr).expect("already validated")
                                }),
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
pub struct NewPartitionKey {
    #[builder(setter(into))]
    alias: String,
    #[builder(setter(into))]
    value: String,
}
impl NewPartitionKey {
    pub fn builder() -> NewPartitionKeyBuilder {
        NewPartitionKeyBuilder::default()
    }
}
impl NewPartitionKeyBuilder {
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
pub struct NewLimit {
    #[builder(setter(strip_option, into), default)]
    timestamp_source: Option<String>,
    balance: Vec<NewBalanceLimit>,
}
impl NewLimit {
    pub fn builder() -> NewLimitBuilder {
        NewLimitBuilder::default()
    }
}
impl NewLimitBuilder {
    fn validate(&self) -> Result<(), String> {
        validate_optional_expression(&self.timestamp_source)
    }
}

#[derive(Clone, Builder, Debug)]
#[builder(build_fn(validate = "Self::validate"))]
pub struct NewBalanceLimit {
    #[builder(setter(into), default)]
    limit_type: BalanceLimitType,
    #[builder(setter(into))]
    layer: String,
    #[builder(setter(into))]
    amount: String,
    #[builder(setter(into))]
    enforcement_direction: String,
    #[builder(setter(into), default)]
    start: Option<String>,
    #[builder(setter(into), default)]
    end: Option<String>,
}
impl NewBalanceLimit {
    pub fn builder() -> NewBalanceLimitBuilder {
        NewBalanceLimitBuilder::default()
    }
}
impl NewBalanceLimitBuilder {
    fn validate(&self) -> Result<(), String> {
        validate_expression(
            self.layer
                .as_ref()
                .expect("Mandatory field 'layer' not set"),
        )?;
        validate_expression(
            self.amount
                .as_ref()
                .expect("Mandatory field 'amount' not set"),
        )?;
        validate_expression(
            self.enforcement_direction
                .as_ref()
                .expect("Mandatory field 'enforcement_direction' not set"),
        )?;
        validate_optional_expression(&self.start)?;
        validate_optional_expression(&self.end)?;
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
