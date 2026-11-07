use cel_interpreter::{CelExpression, CelParseRejection};
use serde::{Deserialize, Serialize};
use tracing::instrument;

use es_entity::*;

pub use crate::dimension::NewPartitionKey;
pub use cala_types::{calculation::*, primitives::*};

#[derive(EsEvent, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
#[serde(tag = "type", rename_all = "snake_case")]
#[es_event(id = "CalculationId", event_context = false)]
pub enum CalculationEvent {
    Initialized {
        values: CalculationValues,
    },
    /// Global only. Rewrites `activation_sequence` to the tip checkpoint at handoff.
    BackfillCompleted {
        activation_sequence: u64,
    },
}

#[derive(EsEntity)]
pub struct Calculation {
    pub id: CalculationId,
    values: CalculationValues,
    events: EntityEvents<CalculationEvent>,
}

impl Calculation {
    pub fn id(&self) -> CalculationId {
        self.values.id
    }

    pub fn values(&self) -> &CalculationValues {
        &self.values
    }

    pub fn into_values(self) -> CalculationValues {
        self.values
    }

    /// Idempotent: `AlreadyApplied` when already `Complete`.
    pub(crate) fn complete_backfill(&mut self, activation_sequence: u64) -> Idempotent<()> {
        if self.values.backfill_status == BackfillStatus::Complete {
            return Idempotent::AlreadyApplied;
        }
        self.values.backfill_status = BackfillStatus::Complete;
        self.values.activation_sequence = activation_sequence;
        self.events.push(CalculationEvent::BackfillCompleted {
            activation_sequence,
        });
        Idempotent::Executed(())
    }
}

impl TryFromEvents<CalculationEvent> for Calculation {
    fn try_from_events(
        events: EntityEvents<CalculationEvent>,
    ) -> Result<Self, EntityHydrationError> {
        let mut hydrator = Calculation::hydrator();
        let mut current: Option<CalculationValues> = None;
        for event in events.iter_all() {
            match event {
                CalculationEvent::Initialized { values } => {
                    current = Some(values.clone());
                }
                CalculationEvent::BackfillCompleted {
                    activation_sequence,
                } => {
                    if let Some(values) = current.as_mut() {
                        values.backfill_status = BackfillStatus::Complete;
                        values.activation_sequence = *activation_sequence;
                    }
                }
            }
        }
        if let Some(values) = current {
            hydrator.id(values.id).values(values);
        }
        hydrator.build(events)
    }
}

/// Representation of a ***new*** calculation entity with required/optional properties and a builder.
#[derive(Debug)]
pub struct NewCalculation {
    pub(super) id: CalculationId,
    pub(super) journal_id: JournalId,
    pub(super) code: String,
    pub(super) name: String,
    description: Option<String>,
    scope: CalculationScope,
    condition: Option<String>,
    dimensions: Vec<NewPartitionKey>,
    effective_date_source: Option<String>,
    history: History,
    activation_sequence: u64,
}

/// Whether a new calculation or attachment folds in the entries that precede it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum History {
    /// Replay history asynchronously; the unit starts `InProgress`.
    #[default]
    Backfill,
    /// The unit is `Complete` at once and covers only later entries.
    FromNow,
}

#[bon::bon]
impl NewCalculation {
    #[builder]
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        #[builder(into)] id: CalculationId,
        #[builder(into)] journal_id: JournalId,
        #[builder(into)] code: String,
        #[builder(into)] name: String,
        #[builder(into)] description: Option<String>,
        #[builder(default)] scope: CalculationScope,
        #[builder(into)] condition: Option<String>,
        #[builder(default)] dimensions: Vec<NewPartitionKey>,
        #[builder(into)] effective_date_source: Option<String>,
        #[builder(default)] history: History,
    ) -> Result<Self, CelParseRejection> {
        validate_optional_expression(&condition)?;
        validate_optional_expression(&effective_date_source)?;
        Ok(Self {
            id,
            journal_id,
            code,
            name,
            description,
            scope,
            condition,
            dimensions,
            effective_date_source,
            history,
            activation_sequence: 0,
        })
    }
}

impl NewCalculation {
    pub(super) fn starts_backfill(&self) -> bool {
        self.scope == CalculationScope::Global && self.history == History::Backfill
    }

    pub(super) fn with_activation_sequence(self, activation_sequence: u64) -> Self {
        Self {
            activation_sequence,
            ..self
        }
    }
}

impl IntoEvents<CalculationEvent> for NewCalculation {
    type IdSource = FromCaller;

    fn into_events(
        self,
        builder: EntityEventsBuilder<CalculationEvent, FromCaller>,
    ) -> EntityEvents<CalculationEvent> {
        let backfill_status = if self.starts_backfill() {
            BackfillStatus::InProgress
        } else {
            BackfillStatus::Complete
        };
        builder.initial_events(
            self.id,
            [CalculationEvent::Initialized {
                values: CalculationValues {
                    id: self.id,
                    version: 1,
                    journal_id: self.journal_id,
                    code: self.code,
                    name: self.name,
                    description: self.description,
                    scope: self.scope,
                    condition: self
                        .condition
                        .map(|expr| CelExpression::try_from(expr).expect("already validated")),
                    dimensions: self
                        .dimensions
                        .into_iter()
                        .map(|input| PartitionKey {
                            alias: input.alias,
                            value: CelExpression::try_from(input.value).expect("already validated"),
                        })
                        .collect(),
                    config: CalculationConfig {
                        effective_date_source: self
                            .effective_date_source
                            .map(|expr| CelExpression::try_from(expr).expect("already validated")),
                    },
                    activation_sequence: self.activation_sequence,
                    backfill_status,
                },
            }],
        )
    }
}

#[instrument(name = "calculation.validate_optional_expression", skip(expr), err(level = tracing::Level::WARN))]
fn validate_optional_expression(expr: &Option<String>) -> Result<(), CelParseRejection> {
    if let Some(expr) = expr {
        CelExpression::try_from(expr.as_str())?;
    }
    Ok(())
}
