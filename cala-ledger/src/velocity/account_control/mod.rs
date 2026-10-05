use es_entity::errlanes::{lanes, Fail};
mod repo;
mod value;

use es_entity::clock::ClockHandle;
use rust_decimal::Decimal;
use sqlx::PgPool;

use cala_types::velocity::{VelocityControlValues, VelocityLimitValues};

use crate::{
    param::Params,
    primitives::{AccountId, DebitOrCredit, Layer},
};

use super::error::AttachVelocityControlRejection;

use repo::*;
pub(crate) use value::*;

#[derive(Clone)]
pub struct AccountControls {
    _pool: PgPool,
    repo: AccountControlRepo,
    clock: ClockHandle,
}

impl AccountControls {
    pub fn new(pool: &PgPool, clock: &ClockHandle) -> Self {
        Self {
            repo: AccountControlRepo::new(pool),
            _pool: pool.clone(),
            clock: clock.clone(),
        }
    }

    pub async fn attach_control_in_op(
        &self,
        db: &mut impl es_entity::AtomicOperation,
        control: &VelocityControlValues,
        account_id: AccountId,
        limits: Vec<VelocityLimitValues>,
        params: impl Into<Params> + std::fmt::Debug,
    ) -> Result<(), Fail<AttachVelocityControlRejection, lanes!(Transient, Fatal)>> {
        let velocity_limits = Self::evaluate_velocity_limits(&self.clock, limits, params.into())?;

        let control = AccountVelocityControl {
            account_id,
            control_id: control.id,
            condition: control.condition.clone(),
            enforcement: control.enforcement.clone(),
            velocity_limits,
        };

        self.repo.create_in_op(db, control).await?;

        Ok(())
    }

    /// Batched counterpart of [`Self::attach_control_in_op`]: attaches the
    /// same `control` (with the same `params`) to every account in
    /// `account_ids` in one round trip.
    ///
    /// `params` is shared across the whole batch (a single
    /// `impl Into<Params>`, not `Vec<Params>`) — every account is attached
    /// to the same control under the same evaluated condition and limits.
    /// Accounts that need different params attach in separate calls.
    ///
    /// Because `params` (and therefore every evaluated `condition` /
    /// `AccountVelocityLimit`) is identical for every account in the
    /// batch, the CEL evaluation that builds `velocity_limits` runs
    /// **once** for the whole batch and is cloned per account — the
    /// per-row difference is only `account_id`.
    #[es_entity::errlanes::instrument(
        level = "debug",
        name = "account_control.attach_control_to_accounts_in_op",
        skip(self, db, limits, params),
        fields(control_id = %control.id, account_count = account_ids.len())
    )]
    pub async fn attach_control_to_accounts_in_op(
        &self,
        db: &mut impl es_entity::AtomicOperation,
        control: &VelocityControlValues,
        account_ids: &[AccountId],
        limits: Vec<VelocityLimitValues>,
        params: impl Into<Params> + std::fmt::Debug,
    ) -> Result<(), Fail<AttachVelocityControlRejection, lanes!(Transient, Fatal)>> {
        if account_ids.is_empty() {
            return Ok(());
        }

        let velocity_limits = Self::evaluate_velocity_limits(&self.clock, limits, params.into())?;

        let controls = account_ids
            .iter()
            .map(|&account_id| AccountVelocityControl {
                account_id,
                control_id: control.id,
                condition: control.condition.clone(),
                enforcement: control.enforcement.clone(),
                velocity_limits: velocity_limits.clone(),
            })
            .collect();

        self.repo.create_all_in_op(db, controls).await?;

        Ok(())
    }

    fn evaluate_velocity_limits(
        clock: &ClockHandle,
        limits: Vec<VelocityLimitValues>,
        params: Params,
    ) -> Result<Vec<AccountVelocityLimit>, AttachVelocityControlRejection> {
        let mut velocity_limits = Vec::new();
        for velocity in limits {
            let defs = velocity.params;
            let ctx = params.clone().into_context(
                clock,
                defs.as_ref(),
                |_, source| AttachVelocityControlRejection::from(source),
                |_, source| AttachVelocityControlRejection::Default(source),
            )?;
            let mut limits = Vec::new();
            for limit in velocity.limit.balance {
                let layer: Layer = limit.layer.try_evaluate(&ctx)?;
                let amount: Decimal = limit.amount.try_evaluate(&ctx)?;
                let enforcement_direction: DebitOrCredit =
                    limit.enforcement_direction.try_evaluate(&ctx)?;
                let start = limit.start.try_evaluate(&ctx)?;
                let end = if let Some(end) = limit.end {
                    Some(end.try_evaluate(&ctx)?)
                } else {
                    None
                };
                limits.push(AccountBalanceLimit {
                    layer,
                    amount,
                    enforcement_direction,
                    start,
                    end,
                })
            }
            velocity_limits.push(AccountVelocityLimit {
                limit_id: velocity.id,
                window: velocity.window,
                condition: velocity.condition,
                currency: velocity.currency,
                limit: AccountLimit {
                    timestamp_source: velocity.limit.timestamp_source,
                    balance: limits,
                },
            });
        }
        Ok(velocity_limits)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cala_types::velocity::{BalanceLimit, Limit, ParamDataType, ParamDefinition};
    use cel_interpreter::{CelConversionRejection, CelExecutionError, CoreTypeCoercion};
    use es_entity::{
        clock::Clock,
        errlanes::{Level, Rejection},
    };
    use std::error::Error;

    #[test]
    fn limit_evaluation_preserves_binding_and_evaluation_causes() {
        let limit = VelocityLimitValues {
            id: crate::primitives::VelocityLimitId::new(),
            name: "test".into(),
            description: "test".into(),
            window: vec![],
            condition: None,
            currency: None,
            params: Some(vec![ParamDefinition {
                name: "amount".into(),
                r#type: ParamDataType::Decimal,
                default: Some("missing_default".parse().unwrap()),
                description: None,
            }]),
            limit: Limit {
                timestamp_source: None,
                balance: vec![BalanceLimit {
                    limit_type: Default::default(),
                    layer: "SETTLED".parse().unwrap(),
                    amount: "params.amount".parse().unwrap(),
                    enforcement_direction: "DEBIT".parse().unwrap(),
                    start: "timestamp('1970-01-01T00:00:00Z')".parse().unwrap(),
                    end: None,
                }],
            },
        };
        let evaluate = |limit, params| {
            AccountControls::evaluate_velocity_limits(Clock::handle(), vec![limit], params)
        };

        let default = evaluate(limit.clone(), Params::new()).unwrap_err();
        assert_eq!(
            <&str>::from(default.code()),
            "CALA_VELOCITY_PARAMETER_DEFAULT_FAILED"
        );
        assert_eq!(default.level(), Level::Info);
        assert!(matches!(&default, AttachVelocityControlRejection::Default(
            CelConversionRejection::UnknownIdent { expression, .. }
        ) if expression == "missing_default"));
        assert!(default.source().unwrap().is::<CelConversionRejection>());
        assert!(default
            .source()
            .unwrap()
            .source()
            .unwrap()
            .is::<CelExecutionError>());

        let mut params = Params::new();
        params.insert("amount", "not a decimal");
        let supplied = evaluate(limit.clone(), params).unwrap_err();
        assert_eq!(
            <&str>::from(supplied.code()),
            "CALA_VELOCITY_PARAMETER_INVALID"
        );
        assert!(matches!(&supplied, AttachVelocityControlRejection::Param(
            cala_types::param::ParamValueRejection::InvalidDecimal { input, .. }
        ) if input == "not a decimal"));
        assert!(supplied
            .source()
            .unwrap()
            .source()
            .unwrap()
            .is::<rust_decimal::Error>());

        let mut params = Params::new();
        params.insert("amount", Decimal::ONE);
        let evaluated = evaluate(limit.clone(), params.clone()).unwrap();
        assert_eq!(evaluated[0].limit.balance[0].amount, Decimal::ONE);

        for (expression, code) in [
            ("missing_amount", "CEL_UNKNOWN_IDENTIFIER"),
            ("true", "CEL_BAD_CORE_TYPE_COERCION"),
        ] {
            let mut limit = limit.clone();
            limit.limit.balance[0].amount = expression.parse().unwrap();
            let field = evaluate(limit, params.clone()).unwrap_err();
            assert_eq!(
                <&str>::from(field.code()),
                "CALA_VELOCITY_LIMIT_EVALUATION_FAILED"
            );
            let source = field
                .source()
                .unwrap()
                .downcast_ref::<CelConversionRejection>()
                .unwrap();
            assert_eq!(<&str>::from(source.code()), code);
            assert_eq!(field.level(), Level::Info);
            assert!(field.source().unwrap().is::<CelConversionRejection>());
            match field {
                AttachVelocityControlRejection::Cel(CelConversionRejection::UnknownIdent {
                    expression: actual,
                    ..
                }) => assert_eq!(actual, expression),
                AttachVelocityControlRejection::Cel(CelConversionRejection::CoreTypeCoercion(
                    CoreTypeCoercion(actual, ..),
                )) => assert_eq!(actual, expression),
                other => panic!("unexpected field rejection: {other:?}"),
            }
        }
    }
}
