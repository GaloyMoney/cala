mod repo;

use chrono::{DateTime, Utc};
use sqlx::PgPool;

use std::collections::HashMap;

use cala_types::{
    account::AccountValues, balance::BalanceSnapshot, entry::EntryValues,
    transaction::TransactionValues,
};

use crate::{
    ledger_operation::*,
    primitives::{AccountId, AccountSetId},
};

use super::{account_control::*, error::*};

use repo::*;

#[derive(Clone)]
pub(super) struct VelocityBalances {
    repo: VelocityBalanceRepo,
}

impl VelocityBalances {
    pub fn new(pool: &PgPool) -> Self {
        Self {
            repo: VelocityBalanceRepo::new(pool),
        }
    }

    pub(crate) async fn update_balances_in_op(
        &self,
        db: &mut LedgerOperation<'_>,
        created_at: DateTime<Utc>,
        transaction: &TransactionValues,
        entries: &[EntryValues],
        controls: HashMap<AccountId, (AccountValues, Vec<AccountVelocityControl>)>,
        account_set_mappings: &HashMap<AccountId, Vec<AccountSetId>>,
    ) -> Result<(), VelocityError> {
        let mut context =
            super::context::EvalContext::new(transaction, controls.values().map(|v| &v.0));

        let entries_to_enforce =
            Self::balances_to_check(&mut context, entries, &controls, account_set_mappings)?;

        if entries_to_enforce.is_empty() {
            return Ok(());
        }

        let current_balances = self
            .repo
            .find_for_update(db, entries_to_enforce.keys())
            .await?;

        let new_balances =
            Self::new_snapshots(context, created_at, current_balances, &entries_to_enforce)?;

        self.repo.insert_new_snapshots(db, new_balances).await?;

        Ok(())
    }

    #[allow(clippy::type_complexity)]
    fn balances_to_check<'a>(
        context: &mut super::context::EvalContext,
        entries: &'a [EntryValues],
        controls: &'a HashMap<AccountId, (AccountValues, Vec<AccountVelocityControl>)>,
        account_set_mappings: &HashMap<AccountId, Vec<AccountSetId>>,
    ) -> Result<
        HashMap<VelocityBalanceKey, Vec<(&'a AccountVelocityLimit, &'a EntryValues)>>,
        VelocityError,
    > {
        let mut balances_to_check: HashMap<
            VelocityBalanceKey,
            Vec<(&AccountVelocityLimit, &EntryValues)>,
        > = HashMap::new();
        for entry in entries {
            let account_id = entry.account_id;
            let mut ids = vec![account_id];
            ids.extend(
                account_set_mappings
                    .get(&account_id)
                    .unwrap_or(&vec![])
                    .iter()
                    .map(AccountId::from)
                    .collect::<Vec<_>>(),
            );
            for account_id in ids {
                let Some((_, controls)) = controls.get(&account_id) else {
                    continue;
                };
                let ctx = context.context_for_entry(account_id, entry);

                for control in controls.iter() {
                    if !control.needs_enforcement(&ctx)? {
                        continue;
                    }
                    for limit in &control.velocity_limits {
                        if let Some(window) = limit.window_for_enforcement(&ctx, entry)? {
                            balances_to_check
                                .entry(VelocityBalanceKey {
                                    window: window,
                                    currency: entry.currency,
                                    journal_id: entry.journal_id,
                                    account_id: account_id,
                                    control_id: control.control_id,
                                    limit_id: limit.limit_id,
                                })
                                .or_default()
                                .push((limit, entry));
                        }
                    }
                }
            }
        }

        Ok(balances_to_check)
    }

    fn new_snapshots<'a>(
        mut context: super::context::EvalContext,
        time: DateTime<Utc>,
        mut current_balances: HashMap<VelocityBalanceKey, Option<BalanceSnapshot>>,
        entries_to_add: &'a HashMap<VelocityBalanceKey, Vec<(&AccountVelocityLimit, &EntryValues)>>,
    ) -> Result<HashMap<&'a VelocityBalanceKey, Vec<BalanceSnapshot>>, VelocityError> {
        let mut res = HashMap::new();

        for (key, entries) in entries_to_add.iter() {
            let mut latest_balance: Option<BalanceSnapshot> = None;
            let mut new_balances = Vec::new();

            for (limit, entry) in entries {
                let ctx = context.context_for_entry(key.account_id, entry);
                let balance = match (latest_balance.take(), current_balances.remove(key)) {
                    (Some(latest), _) => {
                        new_balances.push(latest.clone());
                        latest
                    }
                    (_, Some(Some(balance))) => balance,
                    (_, Some(None)) => {
                        let new_snapshot =
                            crate::balance::Snapshots::new_snapshot(time, entry.account_id, entry);
                        limit.enforce(&ctx, time, &new_snapshot)?;
                        latest_balance = Some(new_snapshot);
                        continue;
                    }
                    _ => unreachable!(),
                };
                let new_snapshot = crate::balance::Snapshots::update_snapshot(time, balance, entry);
                limit.enforce(&ctx, time, &new_snapshot)?;
                latest_balance = Some(new_snapshot);
            }
            if let Some(latest) = latest_balance.take() {
                new_balances.push(latest)
            }
            res.insert(key, new_balances);
        }
        Ok(res)
    }
}
