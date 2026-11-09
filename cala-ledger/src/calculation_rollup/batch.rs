//! The accumulator the tip and the catch-ups land. A transaction's event
//! group may straddle landings (see [`crate::ec_rollup`]); entries are
//! applied straight from the stream when the whole group landed in the
//! batch, and read back through the flush op otherwise.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use obix::{
    out::{EventDelivery, PersistentOutboxEvent},
    EventSequence,
};

use cala_types::{entry::EntryValues, transaction::TransactionValues};

use crate::{
    calculation::{CalculationRollupStats, CalculationRollupTxn, Calculations, RollupScope},
    entry::{Entries, Entry},
    error::CalaFault,
    outbox::OutboxEventPayload,
    primitives::{EntryId, JournalId, TransactionId},
};

/// What the rollup folds from one outbox event.
pub(super) enum Collected {
    Tx(PendingTx),
    Entry(Arc<PersistentOutboxEvent<OutboxEventPayload>>),
}

impl Collected {
    /// `None` for an event the rollup does not fold.
    pub(super) fn from_event(event: &EventDelivery<OutboxEventPayload>) -> Option<Self> {
        match &event.payload {
            Some(OutboxEventPayload::TransactionCreated { transaction }) => {
                Some(Self::Tx(PendingTx {
                    id: transaction.id,
                    sequence: event.sequence,
                    event: event.inner().clone(),
                }))
            }
            Some(OutboxEventPayload::EntryCreated { .. }) => {
                Some(Self::Entry(event.inner().clone()))
            }
            _ => None,
        }
    }

    pub(super) fn journal_id(&self) -> JournalId {
        match self {
            Self::Tx(tx) => tx.transaction().journal_id,
            Self::Entry(event) => entry_of(event).journal_id,
        }
    }
}

/// A transaction pulled from a `TransactionCreated` event, with its outbox
/// sequence. The values are read back through `event`, the shared event the
/// outbox decoded once.
pub(super) struct PendingTx {
    id: TransactionId,
    sequence: EventSequence,
    event: Arc<PersistentOutboxEvent<OutboxEventPayload>>,
}

impl PendingTx {
    fn transaction(&self) -> &TransactionValues {
        match &self.event.payload {
            Some(OutboxEventPayload::TransactionCreated { transaction }) => transaction,
            _ => unreachable!("PendingTx is only built from a TransactionCreated event"),
        }
    }

    fn entry_ids(&self) -> &[EntryId] {
        &self.transaction().entry_ids
    }
}

/// The entry carried by a collected `EntryCreated` event.
fn entry_of(event: &PersistentOutboxEvent<OutboxEventPayload>) -> &EntryValues {
    match &event.payload {
        Some(OutboxEventPayload::EntryCreated { entry }) => entry,
        _ => unreachable!("CalculationRollupBatch::entries only ever holds EntryCreated events"),
    }
}

#[derive(Default)]
pub(crate) struct CalculationRollupBatch {
    txns: Vec<PendingTx>,
    entries: HashMap<TransactionId, Vec<Arc<PersistentOutboxEvent<OutboxEventPayload>>>>,
}

impl CalculationRollupBatch {
    pub(super) fn push(&mut self, collected: Collected) {
        match collected {
            Collected::Tx(tx) => self.txns.push(tx),
            Collected::Entry(event) => self
                .entries
                .entry(entry_of(&event).transaction_id)
                .or_default()
                .push(event),
        }
    }

    pub(super) fn txns_count(&self) -> usize {
        self.txns.len()
    }

    /// Apply the batch under `scope`, reading the entries of split groups
    /// through `op`.
    pub(super) async fn fold_in_op(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        entries: &Entries,
        calculations: &Calculations,
        scope: RollupScope<'_>,
    ) -> Result<CalculationRollupStats, CalaFault> {
        let missing_ids = self.missing_entry_ids();
        let fetched = if missing_ids.is_empty() {
            HashMap::new()
        } else {
            entries.find_all_in_op(&mut *op, &missing_ids).await?
        };
        calculations
            .apply_rollup_in_op(op, self.rollup_txns(&fetched), scope)
            .await
    }

    /// Entry ids that were *not* collected from the stream in this landing.
    fn missing_entry_ids(&self) -> Vec<EntryId> {
        self.txns
            .iter()
            .flat_map(|tx| {
                let collected: HashSet<EntryId> = self
                    .entries
                    .get(&tx.id)
                    .map(|events| events.iter().map(|e| entry_of(e).id).collect())
                    .unwrap_or_default();
                tx.entry_ids()
                    .iter()
                    .copied()
                    .filter(move |id| !collected.contains(id))
            })
            .collect()
    }

    /// The applier's input in landing order, each transaction's entries
    /// topped up from `fetched` and sorted by entry sequence.
    fn rollup_txns<'a>(
        &'a self,
        fetched: &'a HashMap<EntryId, Entry>,
    ) -> Vec<CalculationRollupTxn<'a>> {
        self.txns
            .iter()
            .map(|tx| {
                let entry_ids = tx.entry_ids();
                let mut entry_values: Vec<&EntryValues> = self
                    .entries
                    .get(&tx.id)
                    .map(|events| events.iter().map(|e| entry_of(e)).collect())
                    .unwrap_or_default();
                if entry_values.len() != entry_ids.len() {
                    entry_values.extend(
                        entry_ids
                            .iter()
                            .filter_map(|id| fetched.get(id))
                            .map(Entry::values),
                    );
                }
                entry_values.sort_by_key(|e| e.sequence);

                CalculationRollupTxn {
                    sequence: u64::from(tx.sequence),
                    transaction: tx.transaction(),
                    entries: entry_values,
                }
            })
            .collect()
    }
}
