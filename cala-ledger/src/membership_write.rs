//! The single home of the account-membership *write primitive*: the
//! direct `(account set, account)` edge insert into
//! `cala_account_set_member_accounts` plus its `AccountSetMemberCreated`
//! outbox event, and the per-member (class-2) advisory-lock statement
//! that fences it.
//!
//! It has exactly two callers, with different lock protocols around it:
//!
//! - the **classic attach path** (`account_set` module): arbitrary
//!   accounts, fenced by the full membership lock protocol — coarse
//!   SHARED + per-member EXCLUSIVE (see `ADDVISORY_LOCK_ID` in
//!   `account_set/repo.rs`) — plus the balance-history guard and the
//!   path-uniqueness check;
//! - the **create-inside-set fast path** (`account` module,
//!   `NewAccount::initial_account_sets`): freshly created accounts
//!   joining exactly one set in the same atomic operation, fenced by the
//!   per-member EXCLUSIVE lock alone (the invariant argument for why the
//!   rest is unnecessary lives on `NewAccount::initial_account_sets`).
//!
//! This module is a crate-internal *leaf*: it depends only on
//! `cala_types`, `es_entity`, `sqlx`, and the outbox, and must NOT
//! import anything from `crate::account` or `crate::account_set` — that
//! keeps the module graph a DAG: `account → membership_write ←
//! account_set`.

use thiserror::Error;

use cala_types::account_set::AccountSetMemberId;

use crate::{
    outbox::{OutboxEventPayload, OutboxPublisher},
    primitives::{AccountId, AccountSetId},
};

/// `classid` namespace for the per-member advisory locks (2-arg form),
/// keyed on `hashtext(<member account id>)`. Must stay disjoint from
/// `EC_SET_LOCK_CLASS` (= 1) used by balance locking.
pub(crate) const MEMBER_LOCK_CLASS: i32 = 2;

#[derive(Error, Debug)]
pub(crate) enum MembershipWriteError {
    #[error("MembershipWriteError - Sqlx: {0}")]
    Sqlx(#[from] sqlx::Error),
    #[error("MembershipWriteError - AccountSetsNotFound: {0:?}")]
    AccountSetsNotFound(Vec<AccountSetId>),
}

/// Take the EXCLUSIVE per-member ([`MEMBER_LOCK_CLASS`]) advisory locks
/// for `account_ids`, in one statement.
///
/// Sort and dedup the lock ids in Rust so the per-member locks
/// are always acquired in canonical id order, matching the
/// single-pair path. A SQL ORDER BY is not a reliable
/// substitute: the planner is free to evaluate the lock
/// projection before any sort node.
pub(crate) async fn lock_member_accounts(
    db: &mut impl es_entity::AtomicOperation,
    account_ids: &[AccountId],
) -> Result<(), sqlx::Error> {
    let mut lock_ids = account_ids.to_vec();
    lock_ids.sort();
    lock_ids.dedup();

    sqlx::query!(
        r#"
            SELECT pg_advisory_xact_lock($1, hashtext(v.account_id::text))
            FROM UNNEST($2::uuid[]) AS v(account_id)
            "#,
        MEMBER_LOCK_CLASS,
        &lock_ids as &[AccountId],
    )
    .execute(db.as_executor())
    .await?;
    Ok(())
}

/// Assert that every id in `set_ids` names an existing account set, in
/// one indexed statement. Returns the missing ids
/// ([`MembershipWriteError::AccountSetsNotFound`], sorted for
/// determinism) so each caller maps them to its own typed error.
pub(crate) async fn assert_account_sets_exist(
    db: &mut impl es_entity::AtomicOperation,
    set_ids: &[AccountSetId],
) -> Result<(), MembershipWriteError> {
    let found = sqlx::query!(
        r#"
        SELECT id AS "id: AccountSetId" FROM cala_account_sets WHERE id = ANY($1)
        "#,
        set_ids as &[AccountSetId],
    )
    .fetch_all(db.as_executor())
    .await?;
    if found.len() != set_ids.len() {
        let found: std::collections::HashSet<AccountSetId> =
            found.into_iter().map(|row| row.id).collect();
        let mut missing: Vec<AccountSetId> = set_ids
            .iter()
            .filter(|id| !found.contains(id))
            .copied()
            .collect();
        missing.sort_unstable();
        missing.dedup();
        return Err(MembershipWriteError::AccountSetsNotFound(missing));
    }
    Ok(())
}

/// Insert the direct account-member edges for every `(account_set_id,
/// account_id)` pair — one statement — and publish one
/// [`OutboxEventPayload::AccountSetMemberCreated`] per pair.
///
/// The INSERT text and the event payload are load-bearing: downstream
/// consumers (lana's `ledger_event_mirror`) rely on the events, so both
/// callers must produce byte-identical rows and events.
///
/// Precondition: the caller has taken its path's lock protocol for every
/// member account in this same op (see the module docs).
pub(crate) async fn insert_member_accounts(
    db: &mut impl es_entity::AtomicOperation,
    publisher: &OutboxPublisher,
    pairs: &[(AccountSetId, AccountId)],
) -> Result<(), sqlx::Error> {
    if pairs.is_empty() {
        return Ok(());
    }
    let account_set_ids: Vec<AccountSetId> = pairs.iter().map(|(set_id, _)| *set_id).collect();
    let account_ids: Vec<AccountId> = pairs.iter().map(|(_, account_id)| *account_id).collect();

    // Direct edges only: one insert covers every pair; ancestor sets
    // are resolved by the read-time walk.
    sqlx::query!(
        r#"
          INSERT INTO cala_account_set_member_accounts (account_set_id, member_account_id)
          SELECT account_set_id, account_id
          FROM UNNEST($1::uuid[], $2::uuid[]) AS v(account_set_id, account_id)
          "#,
        &account_set_ids as &[AccountSetId],
        &account_ids as &[AccountId],
    )
    .execute(db.as_executor())
    .await?;

    publisher
        .publish_all(
            db,
            pairs.iter().map(|(account_set_id, account_id)| {
                OutboxEventPayload::AccountSetMemberCreated {
                    account_set_id: *account_set_id,
                    member_id: AccountSetMemberId::Account(*account_id),
                }
            }),
        )
        .await?;

    Ok(())
}
