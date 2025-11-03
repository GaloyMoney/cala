use cel_interpreter::{CelContext, CelExpression};
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use tracing::{field, instrument, Span};

use cala_types::{
    balance::BalanceSnapshot,
    entry::EntryValues,
    velocity::{PartitionKey, VelocityEnforcement, Window},
};

use crate::{
    primitives::{AccountId, Currency, DebitOrCredit, Layer, VelocityControlId, VelocityLimitId},
    velocity::error::*,
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AccountVelocityControl {
    pub account_id: AccountId,
    pub control_id: VelocityControlId,
    pub enforcement: VelocityEnforcement,
    pub condition: Option<CelExpression>,
    pub velocity_limits: Vec<AccountVelocityLimit>,
}

impl AccountVelocityControl {
    pub fn needs_enforcement(&self, ctx: &CelContext) -> Result<bool, VelocityError> {
        if let Some(condition) = &self.condition {
            let result: bool = condition.try_evaluate(ctx)?;
            Ok(result)
        } else {
            Ok(true)
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AccountVelocityLimit {
    pub limit_id: VelocityLimitId,
    pub window: Vec<PartitionKey>,
    pub condition: Option<CelExpression>,
    pub currency: Option<Currency>,
    pub limit: AccountLimit,
}

impl AccountVelocityLimit {
    #[instrument(name = "velocity_limit.window_for_enforcement", skip(self, ctx, entry), fields(limit_id = %self.limit_id, entry_id = %entry.id), err)]
    pub fn window_for_enforcement(
        &self,
        ctx: &CelContext,
        entry: &EntryValues,
    ) -> Result<Option<Window>, VelocityError> {
        if let Some(currency) = &self.currency {
            if currency != &entry.currency {
                return Ok(None);
            }
        }

        if let Some(condition) = &self.condition {
            let result: bool = condition.try_evaluate(ctx)?;
            if !result {
                return Ok(None);
            }
        }

        let mut map = serde_json::Map::new();
        for key in self.window.iter() {
            let value: serde_json::Value = key.value.try_evaluate(ctx)?;
            map.insert(key.alias.clone(), value);
        }

        Ok(Some(map.into()))
    }

    #[instrument(name = "velocity_limit.enforce", skip(self, ctx, snapshot), fields(limit_id = %self.limit_id, account_id = %snapshot.account_id, currency = %snapshot.currency, velocity.limit, velocity.requested, velocity.layer, velocity.direction), err)]
    pub fn enforce(
        &self,
        ctx: &CelContext,
        time: DateTime<Utc>,
        snapshot: &BalanceSnapshot,
    ) -> Result<(), VelocityError> {
        if let Some(currency) = &self.currency {
            if currency != &snapshot.currency {
                return Ok(());
            }
        }
        let time = if let Some(source) = &self.limit.timestamp_source {
            source.try_evaluate(ctx)?
        } else {
            time
        };
        for limit in self.limit.balance.iter() {
            if limit.start > time {
                continue;
            }
            if let Some(end) = limit.end {
                if end <= time {
                    continue;
                }
            }
            let balance =
                crate::balance::BalanceWithDirection::new(limit.enforcement_direction, snapshot);
            let requested = balance.available(limit.layer);

            if requested > limit.amount {
                let err = LimitExceededError {
                    account_id: snapshot.account_id,
                    currency: snapshot.currency,
                    direction: limit.enforcement_direction,
                    limit_id: self.limit_id,
                    layer: limit.layer,
                    limit: limit.amount,
                    requested,
                };
                Span::current().record("velocity.limit", field::display(&err.limit));
                Span::current().record("velocity.requested", field::display(&err.requested));
                Span::current().record("velocity.layer", field::debug(&err.layer));
                Span::current().record("velocity.direction", field::debug(&err.direction));
                return Err(err.into());
            }
        }

        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AccountLimit {
    pub timestamp_source: Option<CelExpression>,
    pub balance: Vec<AccountBalanceLimit>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AccountBalanceLimit {
    pub layer: Layer,
    pub amount: Decimal,
    pub enforcement_direction: DebitOrCredit,
    pub start: DateTime<Utc>,
    pub end: Option<DateTime<Utc>>,
}

#[cfg(test)]
mod tests {
    use crate::primitives::*;

    use super::*;

    #[test]
    fn control_needs_enforcement_when_no_condition_given() {
        let control = AccountVelocityControl {
            account_id: AccountId::new(),
            control_id: VelocityControlId::new(),
            enforcement: VelocityEnforcement::default(),
            condition: None,
            velocity_limits: vec![],
        };
        let ctx = crate::cel_context::initialize();
        assert!(control.needs_enforcement(&ctx).unwrap());
    }

    #[test]
    fn control_needs_enforcement_when_condition_is_true() {
        let mut control = AccountVelocityControl {
            account_id: AccountId::new(),
            control_id: VelocityControlId::new(),
            enforcement: VelocityEnforcement::default(),
            condition: Some("true".parse().unwrap()),
            velocity_limits: vec![],
        };
        let ctx = crate::cel_context::initialize();
        assert!(control.needs_enforcement(&ctx).unwrap());
        control.condition = Some("1 == 2".parse().unwrap());
        assert!(!control.needs_enforcement(&ctx).unwrap());
    }

    fn entry() -> EntryValues {
        EntryValues {
            id: EntryId::new(),
            version: 1,
            transaction_id: TransactionId::new(),
            journal_id: JournalId::new(),
            account_id: AccountId::new(),
            entry_type: "TEST_ENTRY_TYPE".to_string(),
            sequence: 1,
            layer: Layer::Settled,
            currency: "USD".parse().unwrap(),
            direction: DebitOrCredit::Debit,
            units: Decimal::ONE,
            description: None,
            metadata: None,
        }
    }

    #[test]
    fn limit_needs_enforcement_when_no_condition_given() {
        let limit = AccountVelocityLimit {
            limit_id: VelocityLimitId::new(),
            window: vec![],
            condition: None,
            currency: None,
            limit: AccountLimit {
                timestamp_source: None,
                balance: vec![],
            },
        };
        let ctx = crate::cel_context::initialize();
        let entry = entry();
        assert!(limit
            .window_for_enforcement(&ctx, &entry)
            .unwrap()
            .is_some());
    }

    #[test]
    fn limit_does_not_need_enforcement_when_currency_does_not_match() {
        let limit = AccountVelocityLimit {
            limit_id: VelocityLimitId::new(),
            window: vec![],
            condition: None,
            currency: Some("EUR".parse().unwrap()),
            limit: AccountLimit {
                timestamp_source: None,
                balance: vec![],
            },
        };
        let ctx = crate::cel_context::initialize();
        let mut entry = entry();
        assert!(limit
            .window_for_enforcement(&ctx, &entry)
            .unwrap()
            .is_none());

        entry.currency = "EUR".parse().unwrap();
        assert!(limit
            .window_for_enforcement(&ctx, &entry)
            .unwrap()
            .is_some());
    }

    #[test]
    fn limit_needs_enforcement_when_condition_is_true() {
        let mut limit = AccountVelocityLimit {
            limit_id: VelocityLimitId::new(),
            window: vec![],
            currency: None,
            condition: Some("true".parse().unwrap()),
            limit: AccountLimit {
                timestamp_source: None,
                balance: vec![],
            },
        };
        let ctx = crate::cel_context::initialize();
        let entry = entry();
        assert!(limit
            .window_for_enforcement(&ctx, &entry)
            .unwrap()
            .is_some());
        limit.condition = Some("1 == 2".parse().unwrap());
        assert!(limit
            .window_for_enforcement(&ctx, &entry)
            .unwrap()
            .is_none());
    }

    #[test]
    fn limit_interpolates_window() {
        let limit = AccountVelocityLimit {
            limit_id: VelocityLimitId::new(),
            window: vec![
                PartitionKey {
                    alias: "entry_type".to_string(),
                    value: "entry.entryType".parse().unwrap(),
                },
                PartitionKey {
                    alias: "entry_sequence".to_string(),
                    value: "entry.sequence".parse().unwrap(),
                },
            ],
            condition: None,
            currency: None,
            limit: AccountLimit {
                timestamp_source: None,
                balance: vec![],
            },
        };
        let entry = entry();
        let mut ctx = crate::cel_context::initialize();
        ctx.add_variable("entry", &entry);
        let window = limit.window_for_enforcement(&ctx, &entry).unwrap();
        assert_eq!(
            window.unwrap(),
            Window::from(serde_json::json!({
                "entry_type": "TEST_ENTRY_TYPE",
                "entry_sequence": 1
            }))
        );
    }

    #[test]
    fn enforce_restricts_debit() {
        let ctx = crate::cel_context::initialize();
        let time = Utc::now();
        let limit = AccountVelocityLimit {
            limit_id: VelocityLimitId::new(),
            window: vec![],
            currency: None,
            condition: None,
            limit: AccountLimit {
                timestamp_source: None,
                balance: vec![AccountBalanceLimit {
                    layer: Layer::Settled,
                    amount: Decimal::ONE,
                    enforcement_direction: DebitOrCredit::Debit,
                    start: time,
                    end: None,
                }],
            },
        };
        let mut entry = entry();
        let new_snapshot = crate::balance::Snapshots::new_snapshot(time, entry.account_id, &entry);
        let res = limit.enforce(&ctx, time, &new_snapshot);
        assert!(res.is_ok());
        entry.units = Decimal::ONE_HUNDRED;
        let new_snapshot = crate::balance::Snapshots::new_snapshot(time, entry.account_id, &entry);
        let res = limit.enforce(&ctx, time, &new_snapshot);
        assert!(res.is_err());
        entry.direction = DebitOrCredit::Credit;
        let new_snapshot = crate::balance::Snapshots::new_snapshot(time, entry.account_id, &entry);
        let res = limit.enforce(&ctx, time, &new_snapshot);
        assert!(res.is_ok());
    }

    #[test]
    fn enforce_ignores_when_currency_does_not_match() {
        let ctx = crate::cel_context::initialize();
        let time = Utc::now();
        let limit = AccountVelocityLimit {
            limit_id: VelocityLimitId::new(),
            window: vec![],
            currency: Some("EUR".parse().unwrap()),
            condition: None,
            limit: AccountLimit {
                timestamp_source: None,
                balance: vec![AccountBalanceLimit {
                    layer: Layer::Settled,
                    amount: Decimal::ONE,
                    enforcement_direction: DebitOrCredit::Debit,
                    start: time,
                    end: None,
                }],
            },
        };
        let mut entry = entry();
        entry.units = Decimal::ONE_HUNDRED;
        let new_snapshot = crate::balance::Snapshots::new_snapshot(time, entry.account_id, &entry);
        let res = limit.enforce(&ctx, time, &new_snapshot);
        assert!(res.is_ok());
    }

    #[test]
    fn enforce_acts_on_available_balance_per_layer() {
        let ctx = crate::cel_context::initialize();
        let time = Utc::now();
        let limit = AccountVelocityLimit {
            limit_id: VelocityLimitId::new(),
            window: vec![],
            currency: None,
            condition: None,
            limit: AccountLimit {
                timestamp_source: None,
                balance: vec![AccountBalanceLimit {
                    layer: Layer::Pending,
                    amount: Decimal::ONE,
                    enforcement_direction: DebitOrCredit::Debit,
                    start: time,
                    end: None,
                }],
            },
        };
        let mut entry = entry();
        entry.units = Decimal::ONE_HUNDRED;
        entry.layer = Layer::Settled;
        let new_snapshot = crate::balance::Snapshots::new_snapshot(time, entry.account_id, &entry);
        let res = limit.enforce(&ctx, time, &new_snapshot);
        assert!(res.is_err());
        entry.layer = Layer::Pending;
        let new_snapshot = crate::balance::Snapshots::new_snapshot(time, entry.account_id, &entry);
        let res = limit.enforce(&ctx, time, &new_snapshot);
        assert!(res.is_err());

        entry.layer = Layer::Encumbrance;
        let new_snapshot = crate::balance::Snapshots::new_snapshot(time, entry.account_id, &entry);
        let res = limit.enforce(&ctx, time, &new_snapshot);
        assert!(res.is_ok());
    }

    #[test]
    fn enforce_is_time_aware() {
        let mut ctx = crate::cel_context::initialize();
        let time = Utc::now();
        let limit = AccountVelocityLimit {
            limit_id: VelocityLimitId::new(),
            window: vec![],
            currency: None,
            condition: None,
            limit: AccountLimit {
                timestamp_source: Some("time".parse().unwrap()),
                balance: vec![AccountBalanceLimit {
                    layer: Layer::Settled,
                    amount: Decimal::ONE,
                    enforcement_direction: DebitOrCredit::Debit,
                    start: time,
                    end: Some(time + chrono::Duration::minutes(1)),
                }],
            },
        };
        let mut entry = entry();
        entry.units = Decimal::ONE_HUNDRED;
        ctx.add_variable("time", time);
        let new_snapshot = crate::balance::Snapshots::new_snapshot(time, entry.account_id, &entry);
        let res = limit.enforce(&ctx, time, &new_snapshot);
        assert!(res.is_err());
        ctx.add_variable("time", time - chrono::Duration::minutes(1));
        let res = limit.enforce(&ctx, time, &new_snapshot);
        assert!(res.is_ok());
        ctx.add_variable("time", time + chrono::Duration::minutes(2));
        let res = limit.enforce(&ctx, time, &new_snapshot);
        assert!(res.is_ok());
    }

    #[test]
    fn enforce_restricts_credit() {
        let ctx = crate::cel_context::initialize();
        let time = Utc::now();
        let limit = AccountVelocityLimit {
            limit_id: VelocityLimitId::new(),
            window: vec![],
            currency: None,
            condition: None,
            limit: AccountLimit {
                timestamp_source: None,
                balance: vec![AccountBalanceLimit {
                    layer: Layer::Settled,
                    amount: Decimal::ONE,
                    enforcement_direction: DebitOrCredit::Credit,
                    start: time,
                    end: None,
                }],
            },
        };
        let mut entry = entry();
        entry.direction = DebitOrCredit::Credit;
        let new_snapshot = crate::balance::Snapshots::new_snapshot(time, entry.account_id, &entry);
        let res = limit.enforce(&ctx, time, &new_snapshot);
        assert!(res.is_ok());
        entry.units = Decimal::ONE_HUNDRED;
        let new_snapshot = crate::balance::Snapshots::new_snapshot(time, entry.account_id, &entry);
        let res = limit.enforce(&ctx, time, &new_snapshot);
        assert!(res.is_err());
    }
}
