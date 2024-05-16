pub mod error;
mod repo;

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use sqlx::{PgPool, Postgres, Transaction};
use std::collections::HashMap;

use cala_types::{balance::BalanceSnapshot, entry::EntryValues, primitives::*};

use crate::{
    outbox::*,
    primitives::{DataSource, JournalId},
};

use error::BalanceError;
use repo::*;

#[derive(Clone)]
pub struct Balances {
    repo: BalanceRepo,
    outbox: Outbox,
    _pool: PgPool,
}

impl Balances {
    pub(crate) fn new(pool: &PgPool, outbox: Outbox) -> Self {
        Self {
            repo: BalanceRepo::new(pool),
            outbox,
            _pool: pool.clone(),
        }
    }

    pub(crate) async fn update_balances(
        &self,
        mut tx: Transaction<'_, Postgres>,
        created_at: DateTime<Utc>,
        journal_id: JournalId,
        entries: Vec<EntryValues>,
    ) -> Result<Vec<OutboxEventPayload>, BalanceError> {
        let ids = entries
            .iter()
            .map(|entry| (entry.account_id, entry.currency))
            .collect();
        let current_balances = self.repo.find_for_update(&mut tx, journal_id, ids).await?;
        let new_balances = Self::new_snapshots(created_at, current_balances, entries);
        self.repo
            .insert_new_snapshots(&mut tx, journal_id, &new_balances)
            .await?;
        tx.commit().await?;
        Ok(new_balances
            .into_iter()
            .map(|b| {
                if b.version == 1 {
                    OutboxEventPayload::BalanceCreated {
                        source: DataSource::Local,
                        balance: b,
                    }
                } else {
                    OutboxEventPayload::BalanceUpdated {
                        source: DataSource::Local,
                        balance: b,
                    }
                }
            })
            .collect())
    }

    fn new_snapshots(
        time: DateTime<Utc>,
        mut current_balances: HashMap<(AccountId, Currency), BalanceSnapshot>,
        entries: Vec<EntryValues>,
    ) -> Vec<BalanceSnapshot> {
        let mut latest_balances: HashMap<(AccountId, &Currency), BalanceSnapshot> = HashMap::new();
        let mut new_balances = Vec::new();
        for entry in entries.iter() {
            let balance = match (
                latest_balances.remove(&(entry.account_id, &entry.currency)),
                current_balances.remove(&(entry.account_id, entry.currency)),
            ) {
                (Some(latest), _) => {
                    new_balances.push(latest.clone());
                    latest
                }
                (_, Some(balance)) => balance,
                _ => {
                    latest_balances.insert(
                        (entry.account_id, &entry.currency),
                        Self::new_snapshot(time, entry),
                    );
                    continue;
                }
            };
            latest_balances.insert(
                (entry.account_id, &entry.currency),
                Self::update_snapshot(time, balance, entry),
            );
        }
        new_balances.extend(latest_balances.into_values());
        new_balances
    }

    fn new_snapshot(time: DateTime<Utc>, entry: &EntryValues) -> BalanceSnapshot {
        Self::update_snapshot(
            time,
            BalanceSnapshot {
                journal_id: entry.journal_id,
                account_id: entry.account_id,
                entry_id: entry.id,
                currency: entry.currency,
                settled_dr_balance: Decimal::ZERO,
                settled_cr_balance: Decimal::ZERO,
                settled_entry_id: entry.id,
                settled_modified_at: time,
                pending_dr_balance: Decimal::ZERO,
                pending_cr_balance: Decimal::ZERO,
                pending_entry_id: entry.id,
                pending_modified_at: time,
                encumbered_dr_balance: Decimal::ZERO,
                encumbered_cr_balance: Decimal::ZERO,
                encumbered_entry_id: entry.id,
                encumbered_modified_at: time,
                version: 0,
                modified_at: time,
                created_at: time,
            },
            entry,
        )
    }

    fn update_snapshot(
        time: DateTime<Utc>,
        mut snapshot: BalanceSnapshot,
        entry: &EntryValues,
    ) -> BalanceSnapshot {
        snapshot.version += 1;
        snapshot.modified_at = time;
        snapshot.entry_id = entry.id;
        match entry.layer {
            Layer::Settled => {
                snapshot.settled_entry_id = entry.id;
                snapshot.settled_modified_at = time;
                match entry.direction {
                    DebitOrCredit::Debit => {
                        snapshot.settled_dr_balance += entry.units;
                    }
                    DebitOrCredit::Credit => {
                        snapshot.settled_cr_balance += entry.units;
                    }
                }
            }
            Layer::Pending => {
                snapshot.pending_entry_id = entry.id;
                snapshot.pending_modified_at = time;
                match entry.direction {
                    DebitOrCredit::Debit => {
                        snapshot.pending_dr_balance += entry.units;
                    }
                    DebitOrCredit::Credit => {
                        snapshot.pending_cr_balance += entry.units;
                    }
                }
            }
            Layer::Encumbered => {
                snapshot.encumbered_entry_id = entry.id;
                snapshot.encumbered_modified_at = time;
                match entry.direction {
                    DebitOrCredit::Debit => {
                        snapshot.encumbered_dr_balance += entry.units;
                    }
                    DebitOrCredit::Credit => {
                        snapshot.encumbered_cr_balance += entry.units;
                    }
                }
            }
        }
        snapshot
    }

    #[cfg(feature = "import")]
    pub async fn sync_balance_creation(
        &self,
        mut tx: sqlx::Transaction<'_, sqlx::Postgres>,
        origin: DataSourceId,
        balance: BalanceSnapshot,
    ) -> Result<(), BalanceError> {
        self.repo.import_balance(&mut tx, origin, &balance).await?;
        let recorded_at = balance.created_at;
        self.outbox
            .persist_events_at(
                tx,
                std::iter::once(OutboxEventPayload::BalanceCreated {
                    source: DataSource::Remote { id: origin },
                    balance,
                }),
                recorded_at,
            )
            .await?;
        Ok(())
    }

    #[cfg(feature = "import")]
    pub async fn sync_balance_update(
        &self,
        mut tx: sqlx::Transaction<'_, sqlx::Postgres>,
        origin: DataSourceId,
        balance: BalanceSnapshot,
    ) -> Result<(), BalanceError> {
        self.repo
            .import_balance_update(&mut tx, origin, &balance)
            .await?;
        let recorded_at = balance.modified_at;
        self.outbox
            .persist_events_at(
                tx,
                std::iter::once(OutboxEventPayload::BalanceUpdated {
                    source: DataSource::Remote { id: origin },
                    balance,
                }),
                recorded_at,
            )
            .await?;
        Ok(())
    }
}