use sqlx::PgPool;
use tracing::instrument;

use std::collections::HashMap;

use cala_types::{
    balance::BalanceSnapshot,
    outbox::OutboxEventPayload,
    primitives::{
        AccountId, AccountSetId, BalanceId, Currency, DebitOrCredit, EntryId, JournalId, Status,
    },
};

use super::{
    account_balance::AccountBalance,
    cursor::{AccountBalanceByCurrencyCursor, AccountBalanceCursor},
    error::BalanceError,
};
use crate::outbox::OutboxPublisher;

const EC_SET_LOCK_CLASS: i32 = 1;

/// Maximum balance snapshots written per `INSERT` + outbox publish in
/// [`BalanceRepo::insert_new_snapshots`]. A single streaming-rollup batch
/// can fan many transactions into deep ancestor chains; flushing in
/// bounded sub-batches (within the same transaction) keeps any single
/// statement's working set small so it cannot OOM-crash a Postgres backend.
const INSERT_SNAPSHOT_BATCH_SIZE: usize = 5_000;

#[derive(Debug, Clone)]
pub(super) struct BalanceRepo {
    pool: PgPool,
    publisher: OutboxPublisher,
}

impl BalanceRepo {
    pub fn new(pool: &PgPool, publisher: &OutboxPublisher) -> Self {
        Self {
            pool: pool.clone(),
            publisher: publisher.clone(),
        }
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

    #[instrument(name = "balance.find_in_op", skip_all)]
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
            SELECT h.values, a.normal_balance_type AS "normal_balance_type!: DebitOrCredit"
            FROM cala_balance_history h
            JOIN cala_current_balances c
            ON h.journal_id = c.journal_id
            AND h.account_id = c.account_id
            AND h.currency = c.currency
            AND h.version = c.latest_version
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

    #[instrument(name = "balance.find_all", skip_all, err(level = "warn"))]
    pub(super) async fn find_all(
        &self,
        ids: &[BalanceId],
    ) -> Result<HashMap<BalanceId, AccountBalance>, BalanceError> {
        self.find_all_in_op(&self.pool, ids).await
    }

    #[instrument(name = "balance.list_for_account", skip_all, err(level = "warn"))]
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

    #[instrument(name = "balance.list_for_accounts", skip_all, err(level = "warn"))]
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

    #[instrument(name = "balance.find_all_in_op", skip_all, err(level = "warn"))]
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
                    h.values,
                    a.normal_balance_type as "normal_balance_type!: DebitOrCredit"
                FROM cala_balance_history h
                JOIN cala_current_balances c
                    ON h.journal_id = c.journal_id
                    AND h.account_id = c.account_id
                    AND h.currency = c.currency
                    AND h.version = c.latest_version
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

    #[instrument(name = "balance.list_for_account_in_op", skip_all, err(level = "warn"))]
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

    /// Take the poster's per-row locks for a batch of
    /// `(account_id, currency)` pairs and load the current balance
    /// snapshots in two SQL statements (one combined lock query plus a
    /// pure data fetch).
    ///
    /// Two locks are taken per input row:
    ///
    /// - SHARED lock (2-arg `pg_advisory_xact_lock_shared`, classid
    ///   `EC_SET_LOCK_CLASS`) keyed on `account_id`, taken on *every*
    ///   row — leaves and ancestors, EC and non-EC alike. Its
    ///   load-bearing role is the member side of the membership guard:
    ///   `member_has_balance_history_in_op` takes EXCLUSIVE on a member
    ///   before it is added to / removed from a set, so a concurrent
    ///   poster holding SHARED on that member blocks until the guard's
    ///   history check commits. That is what keeps a member from
    ///   joining or leaving a set while it has balance history.
    /// - FOR_UPDATE lock (1-arg `pg_advisory_xact_lock`) keyed on
    ///   `(journal_id, account_id, currency)`, taken only on non-EC
    ///   rows via `CASE WHEN`. Serializes concurrent posters that
    ///   touch the same balance row. Skipped on EC rows because
    ///   posters never write `cala_current_balances` rows for EC
    ///   accounts at all (`find_for_update`'s data fetch filters
    ///   them out), so the lock would always be uncontended there.
    ///
    /// The 2-arg and 1-arg `pg_advisory_xact_lock` namespaces are
    /// disjoint in PostgreSQL, so the two locks cannot collide with
    /// each other. Lock acquisition order across transactions is
    /// canonical because the caller pre-sorts the input via a BTreeSet
    /// in `Balances::update_balances_in_op` and the planner picks a
    /// nested-loop join with `v` as the outer side for the tiny inputs
    /// this query receives, preserving UNNEST scan order through to
    /// the function calls in the SELECT list.
    #[instrument(name = "cala_ledger.balances.find_for_update", skip(self, op))]
    pub(super) async fn find_for_update(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        journal_id: JournalId,
        (account_ids, currencies): &(Vec<AccountId>, Vec<&str>),
    ) -> Result<HashMap<(AccountId, Currency), Option<BalanceSnapshot>>, BalanceError> {
        sqlx::query!(
            r#"
            SELECT
                pg_advisory_xact_lock_shared(
                    $1::int4, hashtext(v.account_id::text)
                ),
                CASE WHEN NOT a.eventually_consistent THEN
                    pg_advisory_xact_lock(
                        hashtext(concat($2::text, v.account_id::text, v.currency))
                    )
                END
            FROM UNNEST($3::uuid[], $4::text[]) AS v(account_id, currency)
            JOIN cala_accounts a ON a.id = v.account_id
            ORDER BY v.account_id, v.currency
            "#,
            EC_SET_LOCK_CLASS,
            journal_id as JournalId,
            account_ids as &[AccountId],
            currencies as &[&str],
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
            JOIN cala_accounts a ON a.id = v.account_id AND a.eventually_consistent = FALSE
            LEFT JOIN cala_current_balances b
                ON b.journal_id = $1
                AND b.account_id = v.account_id
                AND b.currency = v.currency
        "#,
            journal_id as JournalId,
            account_ids as &[AccountId],
            currencies as &[&str]
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

    /// Under a SHARED lock on `parent_account_id` and an EXCLUSIVE
    /// lock on `member_id` (both in the 2-arg EC-set lock namespace,
    /// acquired in a single canonically-ordered SQL statement), return
    /// `true` iff `member_id` has any row in `cala_balance_history`
    /// for `journal_id`.
    ///
    /// The EXCLUSIVE on the member is what makes the existence check
    /// stable: any in-flight poster on `member_id` takes SHARED on it
    /// via `find_for_update`'s combined lock query and blocks against
    /// our EXCLUSIVE, so committed state is fully visible by the time
    /// the `EXISTS` runs.
    ///
    /// The parent lock is SHARED (not EXCLUSIVE) so it stays compatible
    /// with concurrent posters on the same parent — SHARED/SHARED does not
    /// block — which is what keeps multi-call `add_member_in_op`
    /// transactions from contending with posters on hot parent sets.
    #[instrument(
        name = "cala_ledger.balances.member_has_balance_history_in_op",
        skip_all,
        err(level = "warn")
    )]
    pub(super) async fn member_has_balance_history_in_op(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        journal_id: JournalId,
        parent_account_id: AccountId,
        member_id: AccountId,
    ) -> Result<bool, BalanceError> {
        sqlx::query!(
            r#"
            SELECT
                CASE WHEN v.account_id = $2 THEN
                    pg_advisory_xact_lock($1::int4, hashtext(v.account_id::text))
                ELSE
                    pg_advisory_xact_lock_shared($1::int4, hashtext(v.account_id::text))
                END
            FROM UNNEST($3::uuid[]) AS v(account_id)
            ORDER BY v.account_id
            "#,
            EC_SET_LOCK_CLASS,
            member_id as AccountId,
            &[parent_account_id, member_id] as &[AccountId],
        )
        .execute(op.as_executor())
        .await?;

        let row = sqlx::query!(
            r#"
            SELECT EXISTS (
                SELECT 1
                FROM cala_balance_history
                WHERE journal_id = $1 AND account_id = $2
            ) AS "exists!"
            "#,
            journal_id as JournalId,
            member_id as AccountId,
        )
        .fetch_one(op.as_executor())
        .await?;
        Ok(row.exists)
    }

    #[instrument(
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
            journal_id, account_id, currency, latest_version, latest_values, latest_seq
        )
        SELECT
            journal_id,
            account_id,
            currency,
            MAX(version) as latest_version,
            (array_agg(values ORDER BY version DESC))[1] as latest_values,
            MAX(seq) as latest_seq
        FROM new_snapshots
        GROUP BY journal_id, account_id, currency
        ON CONFLICT (account_id, journal_id, currency)
        DO UPDATE SET
            latest_version = GREATEST(c.latest_version, EXCLUDED.latest_version),
            latest_values = CASE
                WHEN c.latest_version < EXCLUDED.latest_version
                THEN EXCLUDED.latest_values
                ELSE c.latest_values
            END,
            latest_seq = GREATEST(c.latest_seq, EXCLUDED.latest_seq)
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

            self.publisher
                .publish_all(
                    op,
                    chunk.iter().map(|balance| {
                        if balance.version == 1 {
                            OutboxEventPayload::BalanceCreated {
                                balance: balance.clone(),
                            }
                        } else {
                            OutboxEventPayload::BalanceUpdated {
                                balance: balance.clone(),
                            }
                        }
                    }),
                )
                .await?;
        }

        Ok(())
    }

    /// For each of `account_ids`, the **eventually-consistent** ancestor
    /// account sets that own it — the streaming rollup's targets. Mirrors
    /// the inline `AccountSetRepo::fetch_mappings_in_op` but keeps only EC
    /// sets: exactly the ones the synchronous poster path deliberately
    /// skips (`find_for_update` filters `eventually_consistent = FALSE`).
    #[instrument(name = "cala_ledger.balances.fetch_ec_set_mappings", skip_all)]
    pub(crate) async fn fetch_ec_set_mappings(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        journal_id: JournalId,
        account_ids: &[AccountId],
    ) -> Result<HashMap<AccountId, Vec<AccountSetId>>, BalanceError> {
        let rows = sqlx::query!(
            r#"
            SELECT
                m.account_set_id AS "account_set_id!: AccountSetId",
                m.member_account_id AS "member_account_id!: AccountId"
            FROM cala_account_set_member_accounts m
            JOIN cala_account_sets s
              ON m.account_set_id = s.id AND s.journal_id = $1
            JOIN cala_accounts a
              ON a.id = m.account_set_id AND a.eventually_consistent = TRUE
            WHERE m.member_account_id = ANY($2)
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

    /// Take the **shared** EC-set advisory lock on `account_ids` (the same
    /// class + ordering as the poster path) and read the current balances
    /// for the requested EC set-accounts. The streaming rollup is the sole
    /// EC-set writer (`spawn_unique`), so this lock is a cheap, defensive
    /// serialization point rather than a hard requirement. Unlike
    /// `find_for_update` this keeps `eventually_consistent = TRUE` rows —
    /// those are exactly the sets the streaming rollup owns.
    #[instrument(name = "cala_ledger.balances.find_ec_balances_for_update", skip_all)]
    pub(crate) async fn find_ec_balances_for_update(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        journal_id: JournalId,
        (account_ids, currencies): &(Vec<AccountId>, Vec<&str>),
    ) -> Result<HashMap<(AccountId, Currency), Option<BalanceSnapshot>>, BalanceError> {
        // Acquire the shared advisory locks in canonical `AccountId` order
        // so overlapping callers serialize without deadlock; ordering is
        // enforced on the input array because the planner may evaluate the
        // per-row lock projection before any SQL-level sort.
        let mut lock_ids: Vec<AccountId> = account_ids.clone();
        lock_ids.sort();
        lock_ids.dedup();
        sqlx::query!(
            r#"
            SELECT pg_advisory_xact_lock_shared($1::int4, hashtext(account_id::text))
            FROM UNNEST($2::uuid[]) AS v(account_id)
            "#,
            EC_SET_LOCK_CLASS,
            &lock_ids as &[AccountId],
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
