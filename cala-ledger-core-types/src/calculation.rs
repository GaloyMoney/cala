use cel_interpreter::CelExpression;
use serde::{Deserialize, Serialize};

pub use crate::dimension::{PartitionKey, Window};
use crate::primitives::*;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
pub struct CalculationValues {
    pub id: CalculationId,
    pub version: u32,
    pub journal_id: JournalId,
    pub code: String,
    pub name: String,
    pub description: Option<String>,
    pub scope: CalculationScope,
    pub condition: Option<CelExpression>,
    pub dimensions: Vec<PartitionKey>,
    pub config: CalculationConfig,
    /// Outbox sequence at creation; the subscriber applies this calculation to
    /// events with a greater sequence only.
    pub activation_sequence: u64,
    /// Only meaningful for `Global` scope; `Attached` calculations are always `Complete`
    /// here and carry status per attachment.
    pub backfill_status: BackfillStatus,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum BackfillStatus {
    /// History is being folded in by the unit's catch-up; the tip ignores
    /// this unit.
    InProgress,
    /// Every entry at or below `activation_sequence` is in the series (or
    /// the unit was created `FromNow`); the tip covers the unit past it.
    Complete,
}

/// Which accounts and sets of the journal a calculation maintains series for.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum CalculationScope {
    /// Every account and account set in the journal.
    Global,
    /// Only the accounts and sets explicitly attached to the calculation.
    #[default]
    Attached,
}

#[derive(Default, Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
pub struct CalculationConfig {
    /// CEL resolving to a date; the effective date the entry's delta lands on.
    /// `None` = the transaction's `effective`.
    pub effective_date_source: Option<CelExpression>,
}

/// One series: the base `BalanceId` plus the calculation and its dimension tuple.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct CalculationBalanceId {
    pub journal_id: JournalId,
    pub calculation_id: CalculationId,
    pub account_id: AccountId,
    pub currency: Currency,
    pub dimensions: Window,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backfill_status_is_required() {
        let stored = serde_json::json!({
            "id": uuid::Uuid::nil(),
            "version": 1,
            "journal_id": uuid::Uuid::nil(),
            "code": "c",
            "name": "n",
            "description": null,
            "scope": "global",
            "condition": null,
            "dimensions": [],
            "config": { "effective_date_source": null },
            "activation_sequence": 7,
        });
        let err = serde_json::from_value::<CalculationValues>(stored).unwrap_err();
        assert!(err.to_string().contains("backfill_status"), "{err}");
    }
}
