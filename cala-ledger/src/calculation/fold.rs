use chrono::{DateTime, NaiveDate, Utc};
use es_entity::errlanes::Rejection;
use std::collections::HashSet;

use cala_types::{
    calculation::{CalculationScope, CalculationValues, Window},
    entry::EntryValues,
    transaction::TransactionValues,
};
use cel_interpreter::{CelContext, CelConversionRejection};

use crate::{
    effective::delta::DeltaAccumulator, error::CalaFault, eval_context::EvalContext, primitives::*,
};

use super::Calculations;

/// One committed transaction of a calculation-rollup batch, with its
/// outbox sequence and its entries in `sequence` order.
pub(crate) struct CalculationRollupTxn<'a> {
    pub sequence: u64,
    pub transaction: &'a TransactionValues,
    pub entries: Vec<&'a EntryValues>,
}

/// What one rollup application did, for the flush span.
#[derive(Debug, Default)]
pub(crate) struct CalculationRollupStats {
    pub deltas: usize,
    pub evaluation_errors: usize,
}

/// Evaluate `calculation` for one entry against one target's context:
/// `None` when the condition is false, otherwise the dimension tuple and
/// the effective date the delta lands on.
fn evaluate(
    calculation: &CalculationValues,
    ctx: &CelContext,
    transaction: &TransactionValues,
) -> Result<Option<(Window, NaiveDate)>, CelConversionRejection> {
    if let Some(condition) = &calculation.condition {
        let matched: bool = condition.try_evaluate(ctx)?;
        if !matched {
            return Ok(None);
        }
    }
    let mut dimensions = serde_json::Map::new();
    for key in calculation.dimensions.iter() {
        let value: serde_json::Value = key.value.try_evaluate(ctx)?;
        dimensions.insert(key.alias.clone(), value);
    }
    let effective = match &calculation.config.effective_date_source {
        Some(source) => source.try_evaluate(ctx)?,
        None => transaction.effective,
    };
    Ok(Some((Window::from(dimensions), effective)))
}

impl Calculations {
    /// Whether any calculation exists in the ledger.
    pub(crate) async fn any_exist_in_op(
        &self,
        op: &mut impl es_entity::AtomicOperation,
    ) -> Result<bool, CalaFault> {
        self.repo.any_exist_in_op(op).await
    }

    /// Fold a batch of committed transactions into the calculation series
    /// they are covered by, per journal, and apply the resulting deltas.
    /// The caller is the calculation rollup, the tables' sole writer.
    #[es_entity::errlanes::instrument(
        level = "debug",
        name = "cala_ledger.calculation.apply_rollup_in_op",
        skip_all,
        fields(txns_count = txns.len())
    )]
    pub(crate) async fn apply_rollup_in_op(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        txns: Vec<CalculationRollupTxn<'_>>,
    ) -> Result<CalculationRollupStats, CalaFault> {
        let mut stats = CalculationRollupStats::default();
        let mut groups: Vec<(JournalId, Vec<CalculationRollupTxn<'_>>)> = Vec::new();
        for tx in txns {
            let journal_id = tx.transaction.journal_id;
            match groups.iter_mut().find(|(j, _)| *j == journal_id) {
                Some((_, group)) => group.push(tx),
                None => groups.push((journal_id, vec![tx])),
            }
        }
        for (journal_id, group) in groups {
            self.apply_rollup_group_in_op(op, journal_id, group, &mut stats)
                .await?;
        }
        Ok(stats)
    }

    async fn apply_rollup_group_in_op(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        journal_id: JournalId,
        group: Vec<CalculationRollupTxn<'_>>,
        stats: &mut CalculationRollupStats,
    ) -> Result<(), CalaFault> {
        let calculations: Vec<CalculationValues> = self
            .repo
            .list_for_journal(&mut *op, journal_id)
            .await?
            .into_iter()
            .map(|c| c.into_values())
            .collect();
        if calculations.is_empty() {
            return Ok(());
        }

        let leaves: Vec<AccountId> = group
            .iter()
            .flat_map(|tx| tx.entries.iter().map(|e| e.account_id))
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();
        let ancestors = self
            .repo
            .fetch_ancestors_in_op(op, journal_id, &leaves)
            .await?;
        let targets: Vec<AccountId> = leaves
            .iter()
            .copied()
            .chain(ancestors.values().flatten().copied())
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();

        let attached_ids: Vec<CalculationId> = calculations
            .iter()
            .filter(|c| c.scope == CalculationScope::Attached)
            .map(|c| c.id)
            .collect();
        let attachments = self
            .attachments
            .find_for_accounts(op, &attached_ids, &targets)
            .await?;
        let account_values = self.repo.account_context_values_in_op(op, &targets).await?;

        let mut deltas = DeltaAccumulator::<(CalculationId, Window)>::new();
        let mut modified_at: Option<DateTime<Utc>> = None;
        let no_ancestors = Vec::new();
        for (tx_index, tx) in group.iter().enumerate() {
            let transaction = tx.transaction;
            modified_at = modified_at.max(Some(transaction.created_at));
            let mut eval =
                EvalContext::new(self.clock.clone(), transaction, account_values.values());
            for entry in tx.entries.iter() {
                let entry_targets = std::iter::once(&entry.account_id)
                    .chain(ancestors.get(&entry.account_id).unwrap_or(&no_ancestors));
                for &target in entry_targets {
                    if !account_values.contains_key(&target) {
                        continue;
                    }
                    let mut ctx: Option<CelContext> = None;
                    for calculation in calculations.iter() {
                        let covered = match calculation.scope {
                            CalculationScope::Global => {
                                tx.sequence > calculation.activation_sequence
                            }
                            CalculationScope::Attached => attachments
                                .get(&(calculation.id, target))
                                .is_some_and(|activation| tx.sequence > *activation),
                        };
                        if !covered {
                            continue;
                        }
                        let ctx = ctx.get_or_insert_with(|| eval.context_for_entry(target, entry));
                        match evaluate(calculation, ctx, transaction) {
                            Ok(Some((window, effective))) => deltas.push_keyed(
                                (calculation.id, window),
                                target,
                                effective,
                                tx_index,
                                entry,
                            ),
                            Ok(None) => {}
                            Err(error) => {
                                stats.evaluation_errors += 1;
                                tracing::error!(
                                    calculation_id = %calculation.id,
                                    entry_id = %entry.id,
                                    transaction_id = %transaction.id,
                                    error_code = <&str>::from(error.code()),
                                    "calculation evaluation failed; entry treated as not matching"
                                );
                            }
                        }
                    }
                }
            }
        }

        if deltas.is_empty() {
            return Ok(());
        }
        let deltas = deltas.into_deltas();
        stats.deltas += deltas.len();
        let modified_at = modified_at.expect("a non-empty group has a created_at");
        self.effective
            .apply_deltas_in_op(op, journal_id, &deltas, modified_at)
            .await
    }
}
