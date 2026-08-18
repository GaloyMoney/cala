use sqlx::PgPool;
use tracing::instrument;

use std::collections::{HashMap, HashSet};

use cala_types::{
    balance::BalanceSnapshot,
    primitives::{
        AccountId, AccountSetId, BalanceId, Currency, DebitOrCredit, EntryId, JournalId, Status,
    },
};

use super::{
    account_balance::AccountBalance,
    cursor::{AccountBalanceByCurrencyCursor, AccountBalanceCursor},
    error::BalanceError,
};

const EC_SET_LOCK_CLASS: i32 = 1;

/// Maximum balance snapshots written per `INSERT` in
/// [`BalanceRepo::insert_new_snapshots`]. A single streaming-rollup batch
/// can fan many transactions into deep ancestor chains; flushing in
/// bounded sub-batches (within the same transaction) keeps any single
/// statement's working set small so it cannot OOM-crash a Postgres backend.
const INSERT_SNAPSHOT_BATCH_SIZE: usize = 5_000;

#[derive(Debug, Clone)]
pub(super) struct BalanceRepo {
    pool: PgPool,
}

impl BalanceRepo {
    pub fn new(pool: &PgPool) -> Self {
        Self { pool: pool.clone() }
    }

    pub async fn find(
        &self,
        journal_id: JournalId,
        account_id: AccountId,
        currency: Currency,
    ) -> Result<AccountBalance, BalanceError> {
        self.find_in_op(&self.pool, journal_id, account_id, currency)
            .await
    }

    #[instrument(level = "debug", name = "balance.find_in_op", skip_all)]
    pub async fn find_in_op(
        &self,
        op: impl es_entity::IntoOneTimeExecutor<'_>,
        journal_id: JournalId,
        account_id: AccountId,
        currency: Currency,
    ) -> Result<AccountBalance, BalanceError> {
        let row = op
            .into_executor()
            .fetch_optional(sqlx::query!(
                r#"
            SELECT c.latest_values AS "values!", a.normal_balance_type AS "normal_balance_type!: DebitOrCredit"
            FROM cala_current_balances c
            JOIN cala_accounts a
            ON c.account_id = a.id
            WHERE c.journal_id = $1
            AND c.account_id = $2
            AND c.currency = $3
            "#,
                journal_id as JournalId,
                account_id as AccountId,
                currency.code(),
            ))
            .await?;

        if let Some(row) = row {
            let details: BalanceSnapshot =
                serde_json::from_value(row.values).expect("Failed to deserialize balance snapshot");
            Ok(AccountBalance::new(row.normal_balance_type, details))
        } else {
            Err(BalanceError::NotFound(journal_id, account_id, currency))
        }
    }

    #[instrument(
        level = "debug",
        name = "balance.find_all",
        skip_all,
        err(level = "warn")
    )]
    pub(super) async fn find_all(
        &self,
        ids: &[BalanceId],
    ) -> Result<HashMap<BalanceId, AccountBalance>, BalanceError> {
        self.find_all_in_op(&self.pool, ids).await
    }

    #[instrument(
        level = "debug",
        name = "balance.list_for_account",
        skip_all,
        err(level = "warn")
    )]
    pub(super) async fn list_for_account(
        &self,
        journal_id: JournalId,
        account_id: AccountId,
        args: es_entity::PaginatedQueryArgs<AccountBalanceByCurrencyCursor>,
    ) -> Result<
        es_entity::PaginatedQueryRet<AccountBalance, AccountBalanceByCurrencyCursor>,
        BalanceError,
    > {
        self.list_for_account_in_op(&self.pool, journal_id, account_id, args)
            .await
    }

    #[instrument(
        level = "debug",
        name = "balance.list_for_accounts",
        skip_all,
        err(level = "warn")
    )]
    pub(super) async fn list_for_accounts(
        &self,
        journal_id: JournalId,
        account_ids: &[AccountId],
        args: es_entity::PaginatedQueryArgs<AccountBalanceCursor>,
    ) -> Result<es_entity::PaginatedQueryRet<AccountBalance, AccountBalanceCursor>, BalanceError>
    {
        self.list_for_accounts_in_op(&self.pool, journal_id, account_ids, args)
            .await
    }

    #[instrument(
        level = "debug",
        name = "balance.find_all_in_op",
        skip_all,
        err(level = "warn")
    )]
    pub(super) async fn find_all_in_op(
        &self,
        op: impl es_entity::IntoOneTimeExecutor<'_>,
        ids: &[BalanceId],
    ) -> Result<HashMap<BalanceId, AccountBalance>, BalanceError> {
        let mut journal_ids = Vec::with_capacity(ids.len());
        let mut account_ids = Vec::with_capacity(ids.len());
        let mut currencies = Vec::with_capacity(ids.len());
        for (journal_id, account_id, currency) in ids {
            journal_ids.push(uuid::Uuid::from(journal_id));
            account_ids.push(uuid::Uuid::from(account_id));
            currencies.push(currency.code().to_string());
        }

        let rows = op
            .into_executor()
            .fetch_all(sqlx::query!(
                r#"
                WITH balance_ids AS (
                    SELECT * FROM UNNEST($1::uuid[], $2::uuid[], $3::text[])
                    AS v(journal_id, account_id, currency)
                )
                SELECT
                    c.latest_values as "values!",
                    a.normal_balance_type as "normal_balance_type!: DebitOrCredit"
                FROM cala_current_balances c
                JOIN cala_accounts a
                    ON c.account_id = a.id
                JOIN balance_ids b
                    ON c.journal_id = b.journal_id
                    AND c.account_id = b.account_id
                    AND c.currency = b.currency"#,
                &journal_ids[..],
                &account_ids[..],
                &currencies[..]
            ))
            .await?;

        let mut ret = HashMap::new();
        for row in rows {
            let details: BalanceSnapshot =
                serde_json::from_value(row.values).expect("Failed to deserialize balance snapshot");
            ret.insert(
                (details.journal_id, details.account_id, details.currency),
                AccountBalance::new(row.normal_balance_type, details),
            );
        }
        Ok(ret)
    }

    #[instrument(
        level = "debug",
        name = "balance.list_for_account_in_op",
        skip_all,
        err(level = "warn")
    )]
    pub(super) async fn list_for_account_in_op(
        &self,
        op: impl es_entity::IntoOneTimeExecutor<'_>,
        journal_id: JournalId,
        account_id: AccountId,
        args: es_entity::PaginatedQueryArgs<AccountBalanceByCurrencyCursor>,
    ) -> Result<
        es_entity::PaginatedQueryRet<AccountBalance, AccountBalanceByCurrencyCursor>,
        BalanceError,
    > {
        let es_entity::PaginatedQueryArgs { first, after } = args;
        let after_currency = after.map(|cursor| cursor.currency.code().to_string());

        let rows = op
            .into_executor()
            .fetch_all(sqlx::query!(
                r#"
                SELECT
                    c.latest_values AS "values!",
                    a.normal_balance_type as "normal_balance_type!: DebitOrCredit"
                FROM cala_current_balances c
                JOIN cala_accounts a
                    ON c.account_id = a.id
                WHERE c.journal_id = $2
                  AND c.account_id = $3
                  AND ($4::text IS NULL OR c.currency > $4)
                ORDER BY c.currency ASC
                LIMIT $1"#,
                (first + 1) as i64,
                journal_id as JournalId,
                account_id as AccountId,
                after_currency.as_deref(),
            ))
            .await?;

        let has_next_page = rows.len() > first;
        let entities = rows
            .into_iter()
            .take(first)
            .map(|row| {
                let details: BalanceSnapshot = serde_json::from_value(row.values)
                    .expect("Failed to deserialize balance snapshot");
                AccountBalance::new(row.normal_balance_type, details)
            })
            .collect::<Vec<_>>();
        let end_cursor = entities.last().map(AccountBalanceByCurrencyCursor::from);

        Ok(es_entity::PaginatedQueryRet {
            entities,
            has_next_page,
            end_cursor,
        })
    }

    #[instrument(
        level = "debug",
        name = "balance.list_for_accounts_in_op",
        skip_all,
        err(level = "warn")
    )]
    pub(super) async fn list_for_accounts_in_op(
        &self,
        op: impl es_entity::IntoOneTimeExecutor<'_>,
        journal_id: JournalId,
        account_ids: &[AccountId],
        args: es_entity::PaginatedQueryArgs<AccountBalanceCursor>,
    ) -> Result<es_entity::PaginatedQueryRet<AccountBalance, AccountBalanceCursor>, BalanceError>
    {
        let es_entity::PaginatedQueryArgs { first, after } = args;
        let (after_account_id, after_currency) = if let Some(after) = after {
            (
                Some(uuid::Uuid::from(after.account_id)),
                Some(after.currency.code().to_string()),
            )
        } else {
            (None, None)
        };

        let rows = op
            .into_executor()
            .fetch_all(sqlx::query!(
                r#"
                WITH account_ids AS (
                    SELECT DISTINCT account_id
                    FROM UNNEST($2::uuid[]) AS v(account_id)
                )
                SELECT
                    c.latest_values AS "values!",
                    a.normal_balance_type as "normal_balance_type!: DebitOrCredit"
                FROM account_ids b
                JOIN cala_current_balances c
                    ON c.account_id = b.account_id
                    AND c.journal_id = $1
                JOIN cala_accounts a
                    ON c.account_id = a.id
                WHERE (
                    $3::uuid IS NULL
                    OR (c.account_id, c.currency) > ($3::uuid, $4::text)
                )
                ORDER BY c.account_id ASC, c.currency ASC
                LIMIT $5"#,
                journal_id as JournalId,
                account_ids as &[AccountId],
                after_account_id,
                after_currency.as_deref(),
                (first + 1) as i64,
            ))
            .await?;

        let has_next_page = rows.len() > first;
        let entities = rows
            .into_iter()
            .take(first)
            .map(|row| {
                let details: BalanceSnapshot = serde_json::from_value(row.values)
                    .expect("Failed to deserialize balance snapshot");
                AccountBalance::new(row.normal_balance_type, details)
            })
            .collect::<Vec<_>>();
        let end_cursor = entities.last().map(AccountBalanceCursor::from);

        Ok(es_entity::PaginatedQueryRet {
            entities,
            has_next_page,
            end_cursor,
        })
    }

    /// Under an EXCLUSIVE lock on `member_id` (2-arg EC-set lock
    /// namespace), return `true` iff `member_id` has any settled
    /// activity in `journal_id` — a row in `cala_balance_history`
    /// **or** a posted `cala_entries` row.
    ///
    /// The `cala_entries` check is load-bearing for eventually-consistent
    /// leaves: an EC leaf writes no `cala_balance_history` inline (the
    /// streaming rollup writes it later), but it writes `cala_entries`
    /// synchronously in the posting transaction — under the same SHARED
    /// member lock. Checking only history would let an EC leaf join or
    /// leave a set *after* posting but *before* the rollup materializes its
    /// history, and the rollup would then fold those pre-/post-membership
    /// entries into the wrong sets. Checking entries closes that window,
    /// bringing EC leaves to parity with synchronous accounts (which write
    /// both). Account sets carry no entries (the set-guard FK forbids it), so
    /// the history check still covers set members.
    ///
    /// The EXCLUSIVE on the member is what makes the existence check
    /// stable: any in-flight poster on `member_id` holds SHARED on it
    /// from *before its first entry insert*
    /// ([`Self::lock_entry_accounts_in_op`]) and blocks against our
    /// EXCLUSIVE, so committed state — including a first posting's
    /// entries — is fully visible by the time the `EXISTS` runs. It
    /// only ever waits on real in-flight activity of the account being
    /// attached, never on unrelated posters.
    ///
    /// No lock is taken on the parent set: the SHARED-on-parent half
    /// that used to be acquired here fenced against the deleted
    /// watermark recalc's EXCLUSIVE-on-parent, and nothing takes an
    /// EXCLUSIVE on a parent set in the streaming design.
    #[instrument(
        level = "debug",
        name = "cala_ledger.balances.member_has_balance_history_in_op",
        skip_all,
        err(level = "warn")
    )]
    pub(super) async fn member_has_balance_history_in_op(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        journal_id: JournalId,
        member_id: AccountId,
    ) -> Result<bool, BalanceError> {
        sqlx::query!(
            r#"
            SELECT pg_advisory_xact_lock($1::int4, hashtext(($2::uuid)::text))
            "#,
            EC_SET_LOCK_CLASS,
            member_id as AccountId,
        )
        .execute(op.as_executor())
        .await?;

        let row = sqlx::query!(
            r#"
            SELECT (
                EXISTS (
                    SELECT 1 FROM cala_balance_history
                    WHERE journal_id = $1 AND account_id = $2
                )
                OR EXISTS (
                    SELECT 1 FROM cala_entries
                    WHERE journal_id = $1 AND account_id = $2
                )
            ) AS "exists!"
            "#,
            journal_id as JournalId,
            member_id as AccountId,
        )
        .fetch_one(op.as_executor())
        .await?;
        Ok(row.exists)
    }

    /// Batch variant of [`member_has_balance_history_in_op`]: takes the
    /// same EXCLUSIVE member locks in a single canonically-ordered
    /// statement and returns every member that already has settled
    /// activity in its journal — balance history **or** a posted entry —
    /// instead of checking pair by pair. See the single-pair method for
    /// why entries are checked (EC leaves write entries synchronously
    /// but history only via the rollup) and why parents are not locked.
    #[instrument(
        level = "debug",
        name = "cala_ledger.balances.members_with_balance_history_in_op",
        skip_all,
        err(level = "warn")
    )]
    pub(super) async fn members_with_balance_history_in_op(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        pairs: &[(JournalId, AccountId)],
    ) -> Result<Vec<AccountId>, BalanceError> {
        if pairs.is_empty() {
            return Ok(Vec::new());
        }
        let journal_ids: Vec<JournalId> = pairs.iter().map(|(j, _)| *j).collect();
        let member_ids: Vec<AccountId> = pairs.iter().map(|(_, m)| *m).collect();

        // Canonical lock protocol, shared with the single-pair
        // member_has_balance_history_in_op: every member id is locked
        // EXCLUSIVE in ascending AccountId order. The ORDER BY makes
        // acquisition order canonical regardless of input order (see
        // the ordering notes on find_for_update); duplicate ids
        // re-acquire the same lock, which is a no-op within one
        // transaction.
        sqlx::query!(
            r#"
            SELECT pg_advisory_xact_lock($1::int4, hashtext(v.account_id::text))
            FROM UNNEST($2::uuid[]) AS v(account_id)
            ORDER BY v.account_id
            "#,
            EC_SET_LOCK_CLASS,
            &member_ids as &[AccountId],
        )
        .execute(op.as_executor())
        .await?;

        let rows = sqlx::query!(
            r#"
            SELECT DISTINCT v.member_id AS "account_id!"
            FROM UNNEST($1::uuid[], $2::uuid[]) AS v(journal_id, member_id)
            WHERE EXISTS (
                SELECT 1 FROM cala_balance_history h
                WHERE h.journal_id = v.journal_id AND h.account_id = v.member_id
            )
            OR EXISTS (
                SELECT 1 FROM cala_entries e
                WHERE e.journal_id = v.journal_id AND e.account_id = v.member_id
            )
            "#,
            &journal_ids as &[JournalId],
            &member_ids as &[AccountId],
        )
        .fetch_all(op.as_executor())
        .await?;
        Ok(rows
            .into_iter()
            .map(|row| AccountId::from(row.account_id))
            .collect())
    }

    #[instrument(
    level = "debug",
    name = "cala_ledger.balances.insert_new_snapshots",
    skip(self, op, new_balances)
    fields(n_new_balances)
)]
    pub(crate) async fn insert_new_snapshots(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        journal_id: JournalId,
        new_balances: Vec<BalanceSnapshot>,
    ) -> Result<(), BalanceError> {
        tracing::Span::current().record(
            "n_new_balances",
            tracing::field::display(new_balances.len()),
        );

        // Flush in bounded sub-batches within the caller's transaction so a
        // large streaming-rollup batch (which can fan many transactions into
        // deep ancestor chains) never becomes one multi-million-row INSERT +
        // outbox publish large enough to OOM-crash a Postgres backend. Each
        // sub-batch is a
        // self-contained statement (history insert + current_balances upsert),
        // so the balance-history FK is satisfied per sub-batch; the whole set
        // still commits atomically as one transaction.
        for chunk in new_balances.chunks(INSERT_SNAPSHOT_BATCH_SIZE) {
            let mut journal_ids = Vec::with_capacity(chunk.len());
            let mut account_ids = Vec::with_capacity(chunk.len());
            let mut entry_ids = Vec::with_capacity(chunk.len());
            let mut currencies = Vec::with_capacity(chunk.len());
            let mut versions = Vec::with_capacity(chunk.len());
            let mut values = Vec::with_capacity(chunk.len());

            for balance in chunk.iter() {
                journal_ids.push(balance.journal_id);
                account_ids.push(balance.account_id);
                entry_ids.push(balance.entry_id);
                currencies.push(balance.currency.code());
                versions.push(balance.version as i32);
                values.push(
                    serde_json::to_value(balance).expect("Failed to serialize balance snapshot"),
                );
            }

            sqlx::query!(
                r#"
        WITH new_snapshots AS (
            INSERT INTO cala_balance_history (
                journal_id, account_id, currency, version, latest_entry_id, values
            )
            SELECT * FROM UNNEST (
                $1::uuid[],
                $2::uuid[],
                $3::text[],
                $4::int4[],
                $5::uuid[],
                $6::jsonb[]
            )
            RETURNING *
        )
        INSERT INTO cala_current_balances AS c (
            journal_id, account_id, currency, latest_version, latest_values
        )
        SELECT
            journal_id,
            account_id,
            currency,
            MAX(version) as latest_version,
            (array_agg(values ORDER BY version DESC))[1] as latest_values
        FROM new_snapshots
        GROUP BY journal_id, account_id, currency
        ON CONFLICT (account_id, journal_id, currency)
        DO UPDATE SET
            latest_version = GREATEST(c.latest_version, EXCLUDED.latest_version),
            latest_values = CASE
                WHEN c.latest_version < EXCLUDED.latest_version
                THEN EXCLUDED.latest_values
                ELSE c.latest_values
            END
        "#,
                &journal_ids as &[JournalId],
                &account_ids as &[AccountId],
                &currencies as &[&str],
                &versions as &[i32],
                &entry_ids as &[EntryId],
                &values
            )
            .execute(op.as_executor())
            .await?;
        }

        Ok(())
    }

    /// For each of `account_ids`, the **eventually-consistent** ancestor
    /// account sets that own it — the streaming rollup's targets. Mirrors
    /// the inline `AccountSetRepo::fetch_mappings_in_op` but keeps only EC
    /// sets: exactly the ones the synchronous poster path deliberately
    /// skips (`find_for_update` filters `eventually_consistent = FALSE`).
    #[instrument(
        level = "debug",
        name = "cala_ledger.balances.fetch_ec_set_mappings",
        skip_all
    )]
    pub(crate) async fn fetch_ec_set_mappings(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        journal_id: JournalId,
        account_ids: &[AccountId],
    ) -> Result<HashMap<AccountId, Vec<AccountSetId>>, BalanceError> {
        // Adjacency-only membership: walk up the set->set edge table from
        // each account's direct memberships, then keep only EC ancestor
        // sets (the streaming rollup's targets). Mirrors
        // `AccountSetRepo::fetch_mappings_in_op` plus the EC filter; UNION
        // dedups and keeps the walk terminating. There is no materialized
        // transitive closure — EC *leaf* accounts are resolved separately
        // by `fetch_ec_leaf_accounts`; this returns only EC ancestor sets.
        let rows = sqlx::query!(
            r#"
            WITH RECURSIVE seed AS (
                SELECT m.member_account_id AS account_id, m.account_set_id
                FROM cala_account_set_member_accounts m
                WHERE m.member_account_id = ANY($2)
            ),
            ancestors AS (
                SELECT account_id, account_set_id FROM seed
                UNION
                SELECT a.account_id, e.account_set_id
                FROM ancestors a
                JOIN cala_account_set_member_account_sets e
                  ON e.member_account_set_id = a.account_set_id
            )
            SELECT
                a.account_set_id AS "account_set_id!: AccountSetId",
                a.account_id AS "member_account_id!: AccountId"
            FROM ancestors a
            JOIN cala_account_sets s
              ON s.id = a.account_set_id AND s.journal_id = $1
            JOIN cala_accounts acc
              ON acc.id = a.account_set_id AND acc.eventually_consistent = TRUE
            "#,
            journal_id as JournalId,
            account_ids as &[AccountId],
        )
        .fetch_all(op.as_executor())
        .await?;

        let mut result: HashMap<AccountId, Vec<AccountSetId>> = HashMap::new();
        for row in rows {
            result
                .entry(row.member_account_id)
                .or_default()
                .push(row.account_set_id);
        }
        Ok(result)
    }

    /// Of `account_ids`, the ones that are **eventually-consistent plain
    /// accounts** (postable leaves): `eventually_consistent = TRUE` and not
    /// backing an account set. These are the leaves the inline poster path
    /// skips (`find_for_update` filters `eventually_consistent = FALSE`), so
    /// the streaming rollup must fold each entry into the leaf's own balance
    /// in addition to its EC ancestor sets.
    #[instrument(
        level = "debug",
        name = "cala_ledger.balances.fetch_ec_leaf_accounts",
        skip_all
    )]
    pub(crate) async fn fetch_ec_leaf_accounts(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        account_ids: &[AccountId],
    ) -> Result<HashSet<AccountId>, BalanceError> {
        let rows = sqlx::query!(
            r#"
            SELECT a.id AS "id!: AccountId"
            FROM cala_accounts a
            WHERE a.id = ANY($1)
              AND a.eventually_consistent = TRUE
              AND a.is_account_set = FALSE
            "#,
            account_ids as &[AccountId],
        )
        .fetch_all(op.as_executor())
        .await?;

        Ok(rows.into_iter().map(|row| row.id).collect())
    }

    /// Take the **shared** EC-set advisory lock on `account_ids` (the same
    /// class + ordering as the poster path) and read the current balances
    /// for the requested EC set-accounts. The streaming rollup is the sole
    /// EC-set writer (a resident job), so this lock is a cheap, defensive
    /// serialization point rather than a hard requirement. Unlike
    /// `find_for_update` this keeps `eventually_consistent = TRUE` rows —
    /// those are exactly the sets the streaming rollup owns.
    #[instrument(
        level = "debug",
        name = "cala_ledger.balances.find_ec_balances_for_update",
        skip_all
    )]
    pub(crate) async fn find_ec_balances_for_update(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        journal_id: JournalId,
        (account_ids, currencies): &(Vec<AccountId>, Vec<&str>),
    ) -> Result<HashMap<(AccountId, Currency), Option<BalanceSnapshot>>, BalanceError> {
        // Acquire the shared advisory locks in canonical `AccountId` order
        // so overlapping callers serialize without deadlock. The `ORDER BY`
        // makes acquisition order canonical regardless of input order (see
        // the ordering notes on `find_for_update`; duplicate ids re-acquire
        // the same lock, which is a no-op within one transaction).
        sqlx::query!(
            r#"
            SELECT pg_advisory_xact_lock_shared($1::int4, hashtext(v.account_id::text))
            FROM UNNEST($2::uuid[]) AS v(account_id)
            ORDER BY v.account_id
            "#,
            EC_SET_LOCK_CLASS,
            account_ids as &[AccountId],
        )
        .execute(op.as_executor())
        .await?;

        let rows = sqlx::query!(
            r#"
            SELECT
                v.account_id AS "account_id!: AccountId",
                v.currency AS "currency!",
                b.latest_values,
                a.status AS "status!: Status"
            FROM UNNEST($2::uuid[], $3::text[]) AS v(account_id, currency)
            JOIN cala_accounts a ON a.id = v.account_id AND a.eventually_consistent = TRUE
            LEFT JOIN cala_current_balances b
                ON b.journal_id = $1
                AND b.account_id = v.account_id
                AND b.currency = v.currency
            "#,
            journal_id as JournalId,
            account_ids as &[AccountId],
            currencies as &[&str],
        )
        .fetch_all(op.as_executor())
        .await?;

        let mut ret = HashMap::new();
        for row in rows {
            if row.status == Status::Locked {
                return Err(BalanceError::AccountLocked(row.account_id));
            }
            let snapshot = row.latest_values.map(|v| {
                serde_json::from_value::<BalanceSnapshot>(v)
                    .expect("Failed to deserialize balance snapshot")
            });
            ret.insert(
                (
                    row.account_id,
                    row.currency.parse().expect("Could not parse currency"),
                ),
                snapshot,
            );
        }
        Ok(ret)
    }
}
