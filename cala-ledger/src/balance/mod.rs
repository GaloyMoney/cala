mod account_balance;
mod effective;
pub mod error;
mod repo;
mod snapshot;

use chrono::{DateTime, NaiveDate, Utc};
use sqlx::PgPool;
use std::collections::{HashMap, HashSet};
use tracing::instrument;

pub use cala_types::{
    balance::{BalanceAmount, BalanceSnapshot},
    journal::JournalValues,
};
use cala_types::{entry::EntryValues, primitives::*};

use crate::{
    journal::Journals,
    ledger_operation::*,
    outbox::*,
    primitives::{DataSource, JournalId},
};

pub use account_balance::*;
use effective::*;
use error::BalanceError;
use repo::*;
pub(crate) use snapshot::*;

#[derive(Clone)]
pub struct Balances {
    repo: BalanceRepo,
    // Used only for "import" feature
    #[allow(dead_code)]
    outbox: Outbox,
    journals: Journals,
    effective: EffectiveBalances,
    _pool: PgPool,
}

impl Balances {
    pub(crate) fn new(pool: &PgPool, outbox: Outbox, journals: &Journals) -> Self {
        Self {
            repo: BalanceRepo::new(pool),
            effective: EffectiveBalances::new(pool),
            outbox,
            journals: journals.clone(),
            _pool: pool.clone(),
        }
    }

    pub fn effective(&self) -> &EffectiveBalances {
        &self.effective
    }

    #[instrument(name = "cala_ledger.balance.find", skip(self), err)]
    pub async fn find(
        &self,
        journal_id: JournalId,
        account_id: impl Into<AccountId> + std::fmt::Debug,
        currency: Currency,
    ) -> Result<AccountBalance, BalanceError> {
        self.repo
            .find(journal_id, account_id.into(), currency)
            .await
    }

    #[instrument(name = "cala_ledger.balance.find_in_op", skip(self, op), err)]
    pub async fn find_in_op(
        &self,
        op: &mut LedgerOperation<'_>,
        journal_id: JournalId,
        account_id: impl Into<AccountId> + std::fmt::Debug,
        currency: Currency,
    ) -> Result<AccountBalance, BalanceError> {
        self.repo
            .find_in_op(op, journal_id, account_id.into(), currency)
            .await
    }

    #[instrument(name = "cala_ledger.balance.find_all", skip(self), err)]
    pub async fn find_all(
        &self,
        ids: &[BalanceId],
    ) -> Result<HashMap<BalanceId, AccountBalance>, BalanceError> {
        self.repo.find_all(ids).await
    }

    pub(crate) async fn update_balances_in_op(
        &self,
        op: &mut LedgerOperation<'_>,
        journal_id: JournalId,
        entries: Vec<EntryValues>,
        effective: NaiveDate,
        created_at: DateTime<Utc>,
        account_set_mappings: HashMap<AccountId, Vec<AccountSetId>>,
    ) -> Result<(), BalanceError> {
        let journal = self.journals.find(journal_id).await?;
        if journal.is_locked() {
            return Err(BalanceError::JournalLocked(journal.id));
        }

        let mut all_involved_balances: HashSet<_> = entries
            .iter()
            .map(|entry| (entry.account_id, entry.currency))
            .collect();
        for entry in entries.iter() {
            if let Some(account_set_ids) = account_set_mappings.get(&entry.account_id) {
                all_involved_balances.extend(
                    account_set_ids
                        .iter()
                        .map(|account_set_id| (AccountId::from(account_set_id), entry.currency)),
                );
            }
        }

        let all_involved_balances: (Vec<_>, Vec<_>) = all_involved_balances
            .into_iter()
            .map(|(a, c)| (a, c.code()))
            .unzip();

        let new_balances = {
            let mut db = op.begin().await?;

            let current_balances = self
                .repo
                .find_for_update(&mut db, journal.id, &all_involved_balances)
                .await?;
            let new_balances = Self::new_snapshots(
                created_at,
                current_balances,
                &entries,
                &account_set_mappings,
            );
            self.repo
                .insert_new_snapshots(&mut db, journal.id, &new_balances)
                .await?;

            if journal.insert_effective_balances() {
                self.effective
                    .update_cumulative_balances_in_op(
                        &mut db,
                        journal_id,
                        entries,
                        effective,
                        created_at,
                        account_set_mappings,
                        all_involved_balances,
                    )
                    .await?;
            }

            db.commit().await?;

            new_balances
        };

        op.accumulate(new_balances.into_iter().map(|balance| {
            if balance.version == 1 {
                OutboxEventPayload::BalanceCreated {
                    source: DataSource::Local,
                    balance,
                }
            } else {
                OutboxEventPayload::BalanceUpdated {
                    source: DataSource::Local,
                    balance,
                }
            }
        }));
        Ok(())
    }

    pub(crate) async fn find_balances_for_update(
        &self,
        db: &mut LedgerOperation<'_>,
        journal_id: JournalId,
        account_id: AccountId,
    ) -> Result<HashMap<Currency, BalanceSnapshot>, BalanceError> {
        self.repo
            .load_all_for_update(db, journal_id, account_id)
            .await
    }

    fn new_snapshots(
        time: DateTime<Utc>,
        mut current_balances: HashMap<(AccountId, Currency), Option<BalanceSnapshot>>,
        entries: &[EntryValues],
        mappings: &HashMap<AccountId, Vec<AccountSetId>>,
    ) -> Vec<BalanceSnapshot> {
        let mut latest_balances: HashMap<(AccountId, &Currency), BalanceSnapshot> = HashMap::new();
        let mut new_balances = Vec::new();
        let empty = Vec::new();
        for entry in entries.iter() {
            for account_id in mappings
                .get(&entry.account_id)
                .unwrap_or(&empty)
                .iter()
                .map(AccountId::from)
                .chain(std::iter::once(entry.account_id))
            {
                let balance = match (
                    latest_balances.remove(&(account_id, &entry.currency)),
                    current_balances.remove(&(account_id, entry.currency)),
                ) {
                    (Some(latest), _) => {
                        new_balances.push(latest.clone());
                        latest
                    }
                    (_, Some(Some(balance))) => balance,
                    (_, Some(None)) => {
                        latest_balances.insert(
                            (account_id, &entry.currency),
                            Snapshots::new_snapshot(time, account_id, entry),
                        );
                        continue;
                    }
                    _ => {
                        continue;
                    }
                };
                latest_balances.insert(
                    (account_id, &entry.currency),
                    Snapshots::update_snapshot(time, balance, entry),
                );
            }
        }
        new_balances.extend(latest_balances.into_values());
        new_balances
    }

    #[cfg(feature = "import")]
    pub async fn sync_balance_creation(
        &self,
        mut db: sqlx::Transaction<'_, sqlx::Postgres>,
        origin: DataSourceId,
        balance: BalanceSnapshot,
    ) -> Result<(), BalanceError> {
        self.repo.import_balance(&mut db, &balance).await?;
        let recorded_at = balance.created_at;
        self.outbox
            .persist_events_at(
                db,
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
        mut db: sqlx::Transaction<'_, sqlx::Postgres>,
        origin: DataSourceId,
        balance: BalanceSnapshot,
    ) -> Result<(), BalanceError> {
        self.repo.import_balance_update(&mut db, &balance).await?;
        let recorded_at = balance.modified_at;
        self.outbox
            .persist_events_at(
                db,
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
