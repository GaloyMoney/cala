//! SQL for the entry-sourced EC verify/repair tool.
//!
//! Kept in its own module (not `balance::repo`) so the reconcile surface
//! stays additive and conflict-free next to ongoing poster-lock work; the
//! only shared item is the `EC_SET_LOCK_CLASS` advisory-lock class.

use std::collections::HashMap;

use chrono::{DateTime, NaiveDate, Utc};

use cala_types::{
    balance::BalanceSnapshot,
    primitives::{AccountId, Currency, JournalId},
};

use super::super::{error::BalanceError, repo::EC_SET_LOCK_CLASS};

/// One leaf entry that a correct streaming applier folded (or would have
/// folded) at or before the anchoring cursor, in applier order.
pub(super) struct ContributingEntry {
    /// Outbox sequence of the transaction's `TransactionCreated` event —
    /// the applier's ordering and the cursor-anchoring key.
    pub tx_seq: i64,
    /// The entry's per-transaction sequence (applier sort key within a
    /// transaction).
    pub entry_seq: i32,
    /// The raw `EntryValues` JSON from the entry's `initialized` event.
    pub entry_values: serde_json::Value,
    /// The posting transaction's `created_at` — the `time` the applier
    /// stamps into every snapshot it folds for this transaction.
    pub tx_created_at: DateTime<Utc>,
    /// The posting transaction's effective date (cumulative-effective
    /// series bucket).
    pub effective: NaiveDate,
}

/// Take **exclusive** advisory locks (EC-set lock class) on `account_ids`.
///
/// This is the reconciler's serialization point against the streaming
/// applier, whose flush takes the SHARED counterpart on every EC account it
/// writes (`find_ec_balances_for_update`) *in the same transaction that
/// advances its checkpoint*. Any in-flight applier batch touching a target
/// therefore blocks here before writing balances *or* advancing the cursor.
///
/// `account_ids` MUST be pre-sorted ascending (and deduped) by the caller:
/// the statement is join-free, so the bare UNNEST scan emits rows in array
/// order and array order IS the lock acquisition order — the same canonical
/// ordering discipline as every other EC-set-class locker.
pub(super) async fn lock_targets_exclusive(
    op: &mut impl es_entity::AtomicOperation,
    account_ids: &[AccountId],
) -> Result<(), BalanceError> {
    sqlx::query!(
        r#"
        SELECT pg_advisory_xact_lock($1::int4, hashtext(v.account_id::text))
        FROM UNNEST($2::uuid[]) AS v(account_id)
        "#,
        EC_SET_LOCK_CLASS,
        account_ids as &[AccountId],
    )
    .execute(op.as_executor())
    .await?;
    Ok(())
}

/// Load the target accounts and validate every one is eventually
/// consistent. Returns the subset of `account_ids` that back an account
/// set (the ones needing downward membership expansion).
pub(super) async fn load_set_backed_targets(
    op: &mut impl es_entity::AtomicOperation,
    account_ids: &[AccountId],
) -> Result<Vec<AccountId>, BalanceError> {
    let rows = sqlx::query!(
        r#"
        SELECT
            a.id AS "id!: AccountId",
            a.eventually_consistent,
            a.is_account_set
        FROM cala_accounts a
        WHERE a.id = ANY($1)
        "#,
        account_ids as &[AccountId],
    )
    .fetch_all(op.as_executor())
    .await?;

    let mut found: HashMap<AccountId, (bool, bool)> = HashMap::new();
    for row in rows {
        found.insert(row.id, (row.eventually_consistent, row.is_account_set));
    }
    let mut set_targets = Vec::new();
    for id in account_ids {
        match found.get(id) {
            Some((true, is_set)) => {
                if *is_set {
                    set_targets.push(*id);
                }
            }
            _ => return Err(BalanceError::NotEventuallyConsistent(*id)),
        }
    }
    Ok(set_targets)
}

/// Read the streaming rollup job's committed cursor from its persisted
/// execution state (`spawn_unique` ⇒ at most one row per job type).
///
/// Returns `0` when the job has never run (no row / no checkpoint yet) —
/// in that state *nothing* has been folded, so the expected EC balances
/// are empty regardless of how many entries exist.
///
/// MUST be called *after* [`lock_targets_exclusive`]: the applier commits
/// its writes and its checkpoint atomically under the SHARED counterpart
/// of our locks, so post-lock the cursor cannot advance for any batch that
/// touches the targets.
pub(super) async fn ec_rollup_cursor(
    op: &mut impl es_entity::AtomicOperation,
    job_type: &str,
) -> Result<i64, BalanceError> {
    let row = sqlx::query!(
        r#"
        SELECT execution_state_json
        FROM job_executions
        WHERE job_type = $1
        "#,
        job_type,
    )
    .fetch_optional(op.as_executor())
    .await?;

    Ok(row
        .and_then(|r| r.execution_state_json)
        .and_then(|state| state.get("sequence").and_then(|s| s.as_i64()))
        .unwrap_or(0))
}

/// Downward membership expansion: for each set-backed target, every
/// account reachable through the set→set edge graph — the inverse of the
/// applier's upward walk (`fetch_ec_set_mappings`). Returns
/// `(target, leaf)` pairs.
///
/// Sound as historical routing because the membership guard
/// (`member_has_balance_history_in_op`) freezes every node on an applied
/// entry's ancestor path: the leaf has entries, and every ancestor —
/// synchronous (inline history) or EC (history at apply time) — has
/// balance history, so no edge on the path can be cut or re-attached.
/// Members that could still move are exactly those with zero applied
/// activity, which contribute nothing to the fold either way.
pub(super) async fn expand_set_targets(
    op: &mut impl es_entity::AtomicOperation,
    set_target_ids: &[AccountId],
) -> Result<Vec<(AccountId, AccountId)>, BalanceError> {
    let rows = sqlx::query!(
        r#"
        WITH RECURSIVE subtree AS (
            SELECT v.target_id AS target_id, v.target_id AS set_id
            FROM UNNEST($1::uuid[]) AS v(target_id)
            UNION
            SELECT s.target_id, e.member_account_set_id
            FROM subtree s
            JOIN cala_account_set_member_account_sets e
                ON e.account_set_id = s.set_id
        )
        SELECT
            s.target_id AS "target_id!: AccountId",
            m.member_account_id AS "leaf_id!: AccountId"
        FROM subtree s
        JOIN cala_account_set_member_accounts m
            ON m.account_set_id = s.set_id
        "#,
        set_target_ids as &[AccountId],
    )
    .fetch_all(op.as_executor())
    .await?;

    Ok(rows
        .into_iter()
        .map(|row| (row.target_id, row.leaf_id))
        .collect())
}

/// One keyset-paginated chunk of the contributing-entry scan.
///
/// Anchors and orders on the **transaction event's** outbox sequence
/// (`≤ cursor`): the applier applies a transaction when its
/// `TransactionCreated` event is processed (entries are stream-collected
/// or DB-loaded), so the transaction event position — never the entry
/// events' — decides whether an entry is folded as of a checkpoint.
/// Entry values are hydrated from the entry's `initialized` event; the
/// entry *set* comes from `cala_entries`, so a transaction whose event
/// group straddled the checkpoint landing is still counted in full.
///
/// The outbox scan is seq-range partition-pruned (#809); the entries side
/// is driven by `idx_cala_entries_account_id`. `after` is the previous
/// chunk's last `(tx_seq, entry_seq)` key.
///
/// NOTE: this depends on persistent-outbox retention covering full
/// history. obix range-partitions but never prunes the persistent outbox
/// today; if pruning/archival ever lands, the reconciler needs an
/// alternative "entries ≤ cursor" mapping.
pub(super) async fn contributing_entries_chunk(
    op: &mut impl es_entity::AtomicOperation,
    journal_id: JournalId,
    cursor: i64,
    leaf_account_ids: &[AccountId],
    after: Option<(i64, i32)>,
    limit: i64,
) -> Result<Vec<ContributingEntry>, BalanceError> {
    let (after_tx_seq, after_entry_seq) = match after {
        Some((tx_seq, entry_seq)) => (Some(tx_seq), Some(entry_seq)),
        None => (None, None),
    };
    let rows = sqlx::query!(
        r#"
        SELECT
            o.sequence AS "tx_seq!",
            (ev.event->'values') AS "entry_values!",
            ((ev.event#>>'{values,sequence}')::int4) AS "entry_seq!",
            t.created_at AS "tx_created_at!",
            t.effective AS "effective!"
        FROM cala_persistent_outbox_events o
        JOIN cala_entries e
            ON e.transaction_id = ((o.payload->'transaction'->>'id')::uuid)
        JOIN cala_entry_events ev
            ON ev.id = e.id AND ev.event_type = 'initialized'
        JOIN cala_transactions t
            ON t.id = e.transaction_id
        WHERE o.payload->>'type' = 'transaction_created'
          AND o.sequence <= $1
          AND e.journal_id = $2
          AND e.account_id = ANY($3)
          AND ($4::bigint IS NULL
               OR (o.sequence, (ev.event#>>'{values,sequence}')::int4)
                  > ($4::bigint, $5::int4))
        ORDER BY o.sequence, (ev.event#>>'{values,sequence}')::int4
        LIMIT $6
        "#,
        cursor,
        journal_id as JournalId,
        leaf_account_ids as &[AccountId],
        after_tx_seq,
        after_entry_seq,
        limit,
    )
    .fetch_all(op.as_executor())
    .await?;

    Ok(rows
        .into_iter()
        .map(|row| ContributingEntry {
            tx_seq: row.tx_seq,
            entry_seq: row.entry_seq,
            entry_values: row.entry_values,
            tx_created_at: row.tx_created_at,
            effective: row.effective,
        })
        .collect())
}

/// The targets' current balance snapshots (`latest_values`), keyed by
/// `(account, currency)`.
pub(super) async fn current_balances(
    op: &mut impl es_entity::AtomicOperation,
    journal_id: JournalId,
    account_ids: &[AccountId],
) -> Result<HashMap<(AccountId, Currency), BalanceSnapshot>, BalanceError> {
    let rows = sqlx::query!(
        r#"
        SELECT
            c.account_id AS "account_id!: AccountId",
            c.currency,
            c.latest_values
        FROM cala_current_balances c
        WHERE c.journal_id = $1
          AND c.account_id = ANY($2)
        "#,
        journal_id as JournalId,
        account_ids as &[AccountId],
    )
    .fetch_all(op.as_executor())
    .await?;

    let mut ret = HashMap::new();
    for row in rows {
        let snapshot: BalanceSnapshot = serde_json::from_value(row.latest_values)
            .expect("Failed to deserialize balance snapshot");
        let currency: Currency = row.currency.parse().expect("Could not parse currency");
        ret.insert((row.account_id, currency), snapshot);
    }
    Ok(ret)
}

/// Highest `cala_balance_history` version per `(account, currency)` for
/// the targets — the floor a corrective snapshot's version must clear even
/// when `cala_current_balances` was corrupted below it (the history
/// version column is unique per balance).
pub(super) async fn max_history_versions(
    op: &mut impl es_entity::AtomicOperation,
    journal_id: JournalId,
    account_ids: &[AccountId],
) -> Result<HashMap<(AccountId, Currency), u32>, BalanceError> {
    let rows = sqlx::query!(
        r#"
        SELECT
            h.account_id AS "account_id!: AccountId",
            h.currency,
            MAX(h.version) AS "max_version!"
        FROM cala_balance_history h
        WHERE h.journal_id = $1
          AND h.account_id = ANY($2)
        GROUP BY h.account_id, h.currency
        "#,
        journal_id as JournalId,
        account_ids as &[AccountId],
    )
    .fetch_all(op.as_executor())
    .await?;

    let mut ret = HashMap::new();
    for row in rows {
        let currency: Currency = row.currency.parse().expect("Could not parse currency");
        ret.insert((row.account_id, currency), row.max_version as u32);
    }
    Ok(ret)
}

/// The latest cumulative-effective snapshot (highest `all_time_version`)
/// per `(account, currency)` for the targets. Its per-layer totals must
/// equal the expected fold's — the cumulative series ends at the same
/// all-time totals the settled balance carries.
pub(super) async fn latest_effective_balances(
    op: &mut impl es_entity::AtomicOperation,
    journal_id: JournalId,
    account_ids: &[AccountId],
) -> Result<HashMap<(AccountId, Currency), BalanceSnapshot>, BalanceError> {
    let rows = sqlx::query!(
        r#"
        SELECT DISTINCT ON (b.account_id, b.currency)
            b.account_id AS "account_id!: AccountId",
            b.currency,
            b.values
        FROM cala_cumulative_effective_balances b
        WHERE b.journal_id = $1
          AND b.account_id = ANY($2)
        ORDER BY b.account_id, b.currency, b.all_time_version DESC
        "#,
        journal_id as JournalId,
        account_ids as &[AccountId],
    )
    .fetch_all(op.as_executor())
    .await?;

    let mut ret = HashMap::new();
    for row in rows {
        let snapshot: BalanceSnapshot =
            serde_json::from_value(row.values).expect("Failed to deserialize balance snapshot");
        let currency: Currency = row.currency.parse().expect("Could not parse currency");
        ret.insert((row.account_id, currency), snapshot);
    }
    Ok(ret)
}
