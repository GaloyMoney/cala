use cel_interpreter::{CelExpression, CelParseRejection};
use serde::{Deserialize, Serialize};
use tracing::instrument;

use es_entity::*;

pub use crate::param::definition::*;
pub use cala_types::{primitives::*, velocity::*};

#[derive(EsEvent, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
#[serde(tag = "type", rename_all = "snake_case")]
#[es_event(id = "VelocityLimitId", event_context = false)]
pub enum VelocityLimitEvent {
    Initialized { values: VelocityLimitValues },
}

#[derive(EsEntity)]
pub struct VelocityLimit {
    pub id: VelocityLimitId,
    values: VelocityLimitValues,
    events: EntityEvents<VelocityLimitEvent>,
}

impl VelocityLimit {
    pub fn id(&self) -> VelocityLimitId {
        self.values.id
    }

    pub fn into_values(self) -> VelocityLimitValues {
        self.values
    }

    pub fn values(&self) -> &VelocityLimitValues {
        &self.values
    }
}

impl TryFromEvents<VelocityLimitEvent> for VelocityLimit {
    fn try_from_events(
        events: EntityEvents<VelocityLimitEvent>,
    ) -> Result<Self, EntityHydrationError> {
        let mut hydrator = VelocityLimit::hydrator();
        for event in events.iter_all() {
            match event {
                VelocityLimitEvent::Initialized { values } => {
                    hydrator.id(values.id).values(values.clone());
                }
            }
        }
        hydrator.build(events)
    }
}

/// Representation of a ***new*** velocity limit entity with required/optional properties and a builder.
#[derive(Debug)]
pub struct NewVelocityLimit {
    pub(super) id: VelocityLimitId,
    pub(super) name: String,
    description: String,
    window: Vec<NewPartitionKey>,
    condition: Option<String>,
    currency: Option<Currency>,
    params: Option<Vec<NewParamDefinition>>,
    limit: NewLimit,
}

#[bon::bon]
impl NewVelocityLimit {
    #[builder]
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        #[builder(into)] id: VelocityLimitId,
        #[builder(into)] name: String,
        #[builder(into)] description: String,
        window: Vec<NewPartitionKey>,
        #[builder(into)] condition: Option<String>,
        #[builder(into)] currency: Option<Currency>,
        params: Option<Vec<NewParamDefinition>>,
        limit: NewLimit,
    ) -> Result<Self, CelParseRejection> {
        validate_optional_expression(&condition)?;
        Ok(Self {
            id,
            name,
            description,
            window,
            condition,
            currency,
            params,
            limit,
        })
    }
}

impl IntoEvents<VelocityLimitEvent> for NewVelocityLimit {
    type IdSource = FromCaller;

    fn into_events(
        self,
        builder: EntityEventsBuilder<VelocityLimitEvent, FromCaller>,
    ) -> EntityEvents<VelocityLimitEvent> {
        let limit = self.limit;
        builder.initial_events(
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
                                start: CelExpression::try_from(input.start)
                                    .expect("already validated"),
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

#[derive(Clone, Debug)]
pub struct NewPartitionKey {
    alias: String,
    value: String,
}

#[bon::bon]
impl NewPartitionKey {
    #[builder]
    pub fn new(
        #[builder(into)] alias: String,
        #[builder(into)] value: String,
    ) -> Result<Self, CelParseRejection> {
        validate_expression(&value)?;
        Ok(Self { alias, value })
    }
}

#[derive(Clone, Debug)]
pub struct NewLimit {
    timestamp_source: Option<String>,
    balance: Vec<NewBalanceLimit>,
}

#[bon::bon]
impl NewLimit {
    #[builder]
    pub fn new(
        #[builder(into)] timestamp_source: Option<String>,
        balance: Vec<NewBalanceLimit>,
    ) -> Result<Self, CelParseRejection> {
        validate_optional_expression(&timestamp_source)?;
        Ok(Self {
            timestamp_source,
            balance,
        })
    }
}

#[derive(Clone, Debug)]
pub struct NewBalanceLimit {
    limit_type: BalanceLimitType,
    layer: String,
    amount: String,
    enforcement_direction: String,
    start: String,
    end: Option<String>,
}

#[bon::bon]
impl NewBalanceLimit {
    #[builder]
    pub fn new(
        #[builder(into, default)] limit_type: BalanceLimitType,
        #[builder(into)] layer: String,
        #[builder(into)] amount: String,
        #[builder(into)] enforcement_direction: String,
        #[builder(into)] start: String,
        #[builder(into)] end: Option<String>,
    ) -> Result<Self, CelParseRejection> {
        validate_expression(&layer)?;
        validate_expression(&amount)?;
        validate_expression(&enforcement_direction)?;
        validate_expression(&start)?;
        validate_optional_expression(&end)?;
        Ok(Self {
            limit_type,
            layer,
            amount,
            enforcement_direction,
            start,
            end,
        })
    }
}

impl<S: new_balance_limit_builder::State> NewBalanceLimitBuilder<S> {
    pub fn always_active(self) -> NewBalanceLimitBuilder<new_balance_limit_builder::SetStart<S>>
    where
        S::Start: new_balance_limit_builder::IsUnset,
    {
        self.start("timestamp('1970-01-01T00:00:00Z')")
    }
}

#[instrument(name = "velocity_limit.validate_expression", skip(expr), fields(expression = %expr), err(level = tracing::Level::WARN))]
fn validate_expression(expr: &str) -> Result<(), CelParseRejection> {
    CelExpression::try_from(expr)?;
    Ok(())
}

#[instrument(name = "velocity_limit.validate_optional_expression", skip(expr), err(level = tracing::Level::WARN))]
fn validate_optional_expression(expr: &Option<String>) -> Result<(), CelParseRejection> {
    if let Some(expr) = expr {
        CelExpression::try_from(expr.as_str())?;
    }
    Ok(())
}
