use chrono::NaiveDate;
use sqlx::{PgPool, Postgres, Transaction};
use std::collections::{HashMap, HashSet};
use tracing::instrument;

use crate::balance::error::BalanceError;
use cala_types::{
    balance::BalanceSnapshot,
    primitives::{AccountId, Currency, JournalId},
};

#[derive(Debug, Clone)]
pub(super) struct EffectiveBalanceRepo {
    _pool: PgPool,
}

impl EffectiveBalanceRepo {
    pub fn new(pool: &PgPool) -> Self {
        Self {
            _pool: pool.clone(),
        }
    }

    #[instrument(
        level = "trace",
        name = "cala_ledger.balances.find_for_update",
        skip(self, db)
    )]
    pub(super) async fn find_for_update(
        &self,
        db: &mut Transaction<'_, Postgres>,
        journal_id: JournalId,
        ids: HashSet<(AccountId, Currency)>,
        effective: NaiveDate,
    ) -> Result<HashMap<(AccountId, Currency), Option<BalanceSnapshot>>, BalanceError> {
        let (account_ids, currencies): (Vec<_>, Vec<_>) =
            ids.into_iter().map(|(a, c)| (a, c.code())).unzip();
        sqlx::query!(
            r#"
          WITH pairs AS (
            SELECT account_id, currency, eventually_consistent
            FROM (
              SELECT * FROM UNNEST($2::uuid[], $3::text[]) AS v(account_id, currency)
            ) AS v
            JOIN cala_accounts a
            ON account_id = a.id
          ),
          delete_balances AS (
            DELETE FROM cala_cumulative_effective_balances
            WHERE journal_id = $1
              AND (account_id, currency) IN (SELECT account_id, currency FROM pairs)
              AND effective >= $4
          ),
          values AS (
            SELECT p.account_id, p.currency, b.values
            FROM pairs p
            LEFT JOIN cala_cumulative_effective_balances b
            ON p.account_id = b.account_id
              AND p.currency = b.currency
            WHERE b.journal_id = $1
              AND b.effective < $4
          )
          SELECT account_id AS "account_id!: AccountId", currency AS "currency!", values FROM values
        "#,
            journal_id as JournalId,
            &account_ids as &[AccountId],
            &currencies as &[&str],
            effective
        )
        .fetch_all(&mut **db)
        .await?;
        unimplemented!()
    }
}
