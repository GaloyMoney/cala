use async_graphql::*;

use super::{
    convert::ToGlobalId,
    primitives::*,
    tx_template::{ParamDefinition, ParamDefinitionInput},
};

#[derive(SimpleObject)]
struct VelocityLimit {
    id: ID,
    velocity_limit_id: UUID,
    name: String,
    description: String,
    condition: Option<Expression>,
    window: Vec<PartitionKey>,
    currency: Option<CurrencyCode>,
    params: Option<Vec<ParamDefinition>>,
    limit: Limit,
}

#[derive(SimpleObject)]
struct Limit {
    timestamp_source: Option<Expression>,
    balance: Vec<BalanceLimit>,
}

#[derive(SimpleObject)]
struct BalanceLimit {
    layer: Expression,
    amount: Expression,
    normal_balance_type: Expression,
    start: Option<Expression>,
    end: Option<Expression>,
}

#[derive(SimpleObject)]
struct PartitionKey {
    alias: String,
    value: Expression,
}

#[derive(InputObject)]
pub(super) struct VelocityLimitCreateInput {
    pub velocity_limit_id: UUID,
    pub name: String,
    pub description: String,
    pub window: Vec<PartitionKeyInput>,
    pub condition: Option<Expression>,
    pub limit: LimitInput,
    pub currency: Option<CurrencyCode>,
    pub params: Option<Vec<ParamDefinitionInput>>,
}

#[derive(InputObject)]
pub(super) struct PartitionKeyInput {
    pub alias: String,
    pub value: Expression,
}

#[derive(InputObject)]
pub(super) struct LimitInput {
    pub timestamp_source: Option<Expression>,
    pub balance: Vec<BalanceLimitInput>,
}

#[derive(InputObject)]
pub(super) struct BalanceLimitInput {
    #[graphql(default)]
    pub limit_type: BalanceLimitType,
    pub layer: Expression,
    pub amount: Expression,
    pub normal_balance_type: Expression,
    pub start: Option<Expression>,
    pub end: Option<Expression>,
}

#[derive(Enum, Default, Copy, Clone, Eq, PartialEq)]
#[graphql(remote = "cala_ledger::velocity::BalanceLimitType")]
pub(super) enum BalanceLimitType {
    #[default]
    Available,
}

#[derive(SimpleObject)]
pub(super) struct VelocityLimitCreatePayload {
    velocity_limit: VelocityLimit,
}

impl ToGlobalId for cala_ledger::VelocityLimitId {
    fn to_global_id(&self) -> async_graphql::types::ID {
        async_graphql::types::ID::from(format!("velocity_limit:{}", self))
    }
}

impl From<cala_ledger::velocity::VelocityLimit> for VelocityLimit {
    fn from(velocity_limit: cala_ledger::velocity::VelocityLimit) -> Self {
        let cala_ledger::velocity::VelocityLimitValues {
            id,
            name,
            description,
            condition,
            currency,
            params,
            limit,
            window,
        } = velocity_limit.into_values();

        let params = params.map(|params| params.into_iter().map(ParamDefinition::from).collect());
        let window = window.into_iter().map(PartitionKey::from).collect();

        Self {
            id: id.to_global_id(),
            velocity_limit_id: UUID::from(id),
            name,
            description,
            condition: condition.map(Expression::from),
            currency: currency.map(CurrencyCode::from),
            params,
            window,
            limit: Limit::from(limit),
        }
    }
}

impl From<cala_ledger::velocity::Limit> for Limit {
    fn from(limit: cala_ledger::velocity::Limit) -> Self {
        let balance = limit.balance.into_iter().map(BalanceLimit::from).collect();

        Self {
            timestamp_source: limit.timestamp_source.map(Expression::from),
            balance,
        }
    }
}

impl From<cala_ledger::velocity::BalanceLimit> for BalanceLimit {
    fn from(balance_limit: cala_ledger::velocity::BalanceLimit) -> Self {
        Self {
            layer: balance_limit.layer.into(),
            amount: balance_limit.amount.into(),
            normal_balance_type: balance_limit.enforcement_direction.into(),
            start: balance_limit.start.map(Expression::from),
            end: balance_limit.end.map(Expression::from),
        }
    }
}

impl From<cala_ledger::velocity::PartitionKey> for PartitionKey {
    fn from(partition_key: cala_ledger::velocity::PartitionKey) -> Self {
        Self {
            alias: partition_key.alias,
            value: Expression::from(partition_key.value),
        }
    }
}

impl From<cala_ledger::velocity::VelocityLimit> for VelocityLimitCreatePayload {
    fn from(entity: cala_ledger::velocity::VelocityLimit) -> Self {
        Self {
            velocity_limit: VelocityLimit::from(entity),
        }
    }
}
