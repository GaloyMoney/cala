use chrono::{DateTime, Utc};
use es_entity::*;
use sqlx::PgPool;
use tracing::instrument;

use std::collections::HashMap;

use crate::{
    outbox::OutboxPublisher,
    primitives::{AccountId, JournalId},
};

use super::{entity::*, error::*};

/// `classid` namespace for the per-set hierarchy advisory locks (2-arg
/// form), keyed on `hashtext(<account set id>)`. Must stay disjoint
/// from `EC_SET_LOCK_CLASS` (= 1) used by balance locking.
///
/// Membership maintenance is a read-then-write over an ancestor chain:
/// each mutation walks the set-to-set edges upward (the recursive
/// `parents` CTE) and then writes the transitive-closure rows the walk
/// justifies. Two concurrent mutations that each miss the other's
/// uncommitted writes would leave the closure table inconsistent
/// (write-skew), so every mutation locks its whole scope — the target
/// set(s) plus all their ancestors — before walking, and the locks are
/// transaction-scoped so the walk's snapshot stays valid until the
/// writes commit.
///
/// Lock modes:
///
/// - Set-structure mutations (`add_member_set` / `remove_member_set`)
///   take EXCLUSIVE locks on their scope. They mutate the edges that
///   every walk reads, and read the member rows that account-member
///   mutations write, so they must exclude everything whose scope
///   overlaps theirs.
/// - Account-member mutations (`add_member_account` /
///   `remove_member_account`) take SHARED locks on their scope plus an
///   EXCLUSIVE per-member lock ([`MEMBER_LOCK_CLASS`]). Account-member
///   mutations only read the edges, and their closure writes for
///   different members are disjoint rows, so they can share a scope;
///   the per-member lock serializes mutations touching the *same*
///   member (e.g. an add and a remove in overlapping hierarchies),
///   whose interleaved inserts/deletes on shared ancestors would
///   otherwise tear the closure.
///
/// Mutations on disjoint hierarchies take disjoint locks and no longer
/// contend at all (previously a single global advisory lock serialized
/// every membership mutation ledger-wide).
const SET_HIERARCHY_LOCK_CLASS: i32 = 2;

/// `classid` namespace for the per-member advisory locks (2-arg form),
/// keyed on `hashtext(<member account id>)`. See
/// [`SET_HIERARCHY_LOCK_CLASS`].
const MEMBER_LOCK_CLASS: i32 = 3;

/// Bound on lock/re-walk rounds in [`lock_membership_scope`]. Each
/// extra round requires a concurrent structure mutation to have grown
/// the ancestor chain mid-acquisition, so in practice one verification
/// round suffices; the bound only guards against livelock under
/// pathological structure churn.
const MAX_LOCK_ROUNDS: usize = 5;

/// Locks the membership-mutation scope: `seed_ids` plus all their
/// ancestors (via [`SET_HIERARCHY_LOCK_CLASS`] locks, SHARED unless
/// `exclusive`), then `member_account_id` if given (EXCLUSIVE
/// [`MEMBER_LOCK_CLASS`] lock).
///
/// The chain is discovered by walking the edges, but the walk itself
/// needs the locks to be stable — so acquisition loops: walk, lock the
/// newly discovered nodes (in canonical lock-key order), re-walk, and
/// finish once a walk discovers nothing unlocked. A re-walk can only
/// differ if a concurrent structure mutation committed an edge into the
/// chain between our walk and our lock acquisition; once every node of
/// the current chain is locked, further structure mutations touching it
/// block on us and the chain can no longer change.
///
/// Locks acquired in later rounds (and across multiple membership calls
/// in one transaction) do not follow the canonical order, so conflicting
/// acquisitions can in rare cases deadlock instead of queueing; Postgres
/// detects and aborts one transaction (SQLSTATE 40P01), which callers
/// should treat as retryable. The per-member lock is acquired last —
/// never wait on chain locks while holding a member lock other
/// transactions may queue on.
async fn lock_membership_scope(
    db: &mut impl es_entity::AtomicOperation,
    seed_ids: &[AccountSetId],
    member_account_id: Option<AccountId>,
    exclusive: bool,
) -> Result<(), AccountSetError> {
    let mut held: std::collections::HashSet<i32> = std::collections::HashSet::new();
    let mut stable = false;
    for _ in 0..MAX_LOCK_ROUNDS {
        let chain = sqlx::query!(
            r#"
            WITH RECURSIVE parents AS (
              SELECT m.account_set_id
              FROM cala_account_set_member_account_sets m
              WHERE m.member_account_set_id = ANY($1)

              UNION
              SELECT m.account_set_id
              FROM parents p
              JOIN cala_account_set_member_account_sets m
                ON m.member_account_set_id = p.account_set_id
            )
            SELECT DISTINCT hashtext(node_id::text) AS "lock_key!"
            FROM (
              SELECT account_set_id AS node_id FROM parents
              UNION
              SELECT UNNEST($1::uuid[])
            ) nodes
            "#,
            seed_ids as &[AccountSetId],
        )
        .fetch_all(db.as_executor())
        .await?;

        let mut missing: Vec<i32> = chain
            .into_iter()
            .map(|row| row.lock_key)
            .filter(|key| !held.contains(key))
            .collect();
        if missing.is_empty() {
            stable = true;
            break;
        }
        // Canonical order within the batch so concurrent overlapping
        // acquisitions queue instead of deadlocking. Sorting has to
        // happen on the input array: the planner is free to evaluate
        // the lock calls before any SQL-level sort node.
        missing.sort_unstable();
        sqlx::query!(
            r#"
            SELECT CASE
              WHEN $1 THEN pg_advisory_xact_lock($2, v.lock_key)
              ELSE pg_advisory_xact_lock_shared($2, v.lock_key)
            END
            FROM UNNEST($3::int4[]) AS v(lock_key)
            "#,
            exclusive,
            SET_HIERARCHY_LOCK_CLASS,
            &missing as &[i32],
        )
        .execute(db.as_executor())
        .await?;
        held.extend(missing);
    }
    if !stable {
        return Err(AccountSetError::HierarchyLockContention);
    }

    if let Some(account_id) = member_account_id {
        sqlx::query!(
            "SELECT pg_advisory_xact_lock($1, hashtext($2))",
            MEMBER_LOCK_CLASS,
            account_id.to_string(),
        )
        .execute(db.as_executor())
        .await?;
    }
    Ok(())
}

pub mod members_cursor {
    use cala_types::account_set::{
        AccountSetMember, AccountSetMemberByExternalId, AccountSetMemberId,
    };
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Serialize, Deserialize)]
    pub struct AccountSetMemberByCreatedAtCursor {
        pub id: AccountSetMemberId,
        pub member_created_at: chrono::DateTime<chrono::Utc>,
    }

    impl From<&AccountSetMember> for AccountSetMemberByCreatedAtCursor {
        fn from(member: &AccountSetMember) -> Self {
            Self {
                id: member.id,
                member_created_at: member.created_at,
            }
        }
    }

    #[derive(Debug, Serialize, Deserialize)]
    pub struct AccountSetMemberByExternalIdCursor {
        pub id: AccountSetMemberId,
        pub external_id: Option<String>,
    }

    impl From<&AccountSetMemberByExternalId> for AccountSetMemberByExternalIdCursor {
        fn from(member: &AccountSetMemberByExternalId) -> Self {
            Self {
                id: member.id,
                external_id: member.external_id.clone(),
            }
        }
    }
}

use account_set_cursor::*;
use members_cursor::*;

#[derive(EsRepo, Debug, Clone)]
#[es_repo(
    entity = "AccountSet",
    columns(
        name(
            ty = "String",
            update(accessor = "values().name"),
            list_by,
            list_for(by(created_at))
        ),
        journal_id(ty = "JournalId", update(persist = false)),
        external_id(
            ty = "Option<String>",
            update(accessor = "values().external_id"),
            list_by
        ),
    ),
    tbl_prefix = "cala",
    post_persist_hook = "publish",
    persist_event_context = false
)]
pub(super) struct AccountSetRepo {
    pool: PgPool,
    publisher: OutboxPublisher,
}

impl AccountSetRepo {
    pub fn new(pool: &PgPool, publisher: &OutboxPublisher) -> Self {
        Self {
            pool: pool.clone(),
            publisher: publisher.clone(),
        }
    }

    pub async fn list_children_by_created_at(
        &self,
        id: AccountSetId,
        args: es_entity::PaginatedQueryArgs<AccountSetMemberByCreatedAtCursor>,
    ) -> Result<
        es_entity::PaginatedQueryRet<AccountSetMember, AccountSetMemberByCreatedAtCursor>,
        AccountSetError,
    > {
        self.list_children_by_created_at_in_op(&self.pool, id, args)
            .await
    }

    #[instrument(
        name = "account_set.list_children_by_created_at_in_op",
        skip_all,
        err(level = "warn")
    )]
    pub async fn list_children_by_created_at_in_op(
        &self,
        op: impl es_entity::IntoOneTimeExecutor<'_>,
        account_set_id: AccountSetId,
        args: es_entity::PaginatedQueryArgs<AccountSetMemberByCreatedAtCursor>,
    ) -> Result<
        es_entity::PaginatedQueryRet<AccountSetMember, AccountSetMemberByCreatedAtCursor>,
        AccountSetError,
    > {
        let es_entity::PaginatedQueryArgs { first, after } = args;
        let (member_id, created_at) = if let Some(after) = after {
            (Some(after.id), Some(after.member_created_at))
        } else {
            (None, None)
        };

        let id = match member_id {
            Some(member_id) => match member_id {
                AccountSetMemberId::Account(id) => Some(id),
                AccountSetMemberId::AccountSet(id) => Some(id.into()),
            },
            None => None,
        };

        let rows = op
            .into_executor()
            .fetch_all(sqlx::query!(
                r#"
            WITH member_accounts AS (
              SELECT
                member_account_id AS member_id,
                member_account_id,
                NULL::uuid AS member_account_set_id,
                created_at
              FROM cala_account_set_member_accounts
              WHERE
                transitive IS FALSE
                AND account_set_id = $4
                AND (COALESCE((created_at, member_account_id) < ($3, $2), $2 IS NULL))
              ORDER BY created_at DESC, member_account_id DESC
              LIMIT $1
            ), member_sets AS (
              SELECT
                member_account_set_id AS member_id,
                NULL::uuid AS member_account_id,
                member_account_set_id,
                created_at
              FROM cala_account_set_member_account_sets
              WHERE
                account_set_id = $4
                AND (COALESCE((created_at, member_account_set_id) < ($3, $2), $2 IS NULL))
              ORDER BY created_at DESC, member_account_set_id DESC
              LIMIT $1
            ), all_members AS (
              SELECT * FROM member_accounts
              UNION ALL
              SELECT * FROM member_sets
            )
            SELECT * FROM all_members
            ORDER BY created_at DESC, member_id DESC
            LIMIT $1
          "#,
                (first + 1) as i64,
                id.map(uuid::Uuid::from),
                created_at,
                uuid::Uuid::from(account_set_id),
            ))
            .await?;
        let has_next_page = rows.len() > first;
        let mut end_cursor = None;
        if let Some(last) = rows.last() {
            let id = last
                .member_account_id
                .map(|account_id| AccountSetMemberId::Account(account_id.into()))
                .or_else(|| {
                    last.member_account_set_id
                        .map(|account_set_id| AccountSetMemberId::AccountSet(account_set_id.into()))
                });
            end_cursor = Some(AccountSetMemberByCreatedAtCursor {
                id: id.expect("member_id not set"),
                member_created_at: last.created_at.expect("created_at not set"),
            });
        }

        let account_set_members = rows
            .into_iter()
            .take(first)
            .map(
                |row| match (row.member_account_id, row.member_account_set_id) {
                    (Some(member_account_id), _) => AccountSetMember::from((
                        AccountSetMemberId::Account(AccountId::from(member_account_id)),
                        row.created_at.expect("created at should always be present"),
                    )),
                    (_, Some(member_account_set_id)) => AccountSetMember::from((
                        AccountSetMemberId::AccountSet(AccountSetId::from(member_account_set_id)),
                        row.created_at.expect("created at should always be present"),
                    )),
                    _ => unreachable!(),
                },
            )
            .collect::<Vec<AccountSetMember>>();

        Ok(es_entity::PaginatedQueryRet {
            entities: account_set_members,
            has_next_page,
            end_cursor,
        })
    }

    pub async fn list_children_by_external_id(
        &self,
        id: AccountSetId,
        args: es_entity::PaginatedQueryArgs<AccountSetMemberByExternalIdCursor>,
    ) -> Result<
        es_entity::PaginatedQueryRet<
            AccountSetMemberByExternalId,
            AccountSetMemberByExternalIdCursor,
        >,
        AccountSetError,
    > {
        self.list_children_by_external_id_in_op(&self.pool, id, args)
            .await
    }

    pub async fn list_children_by_external_id_in_op(
        &self,
        op: impl es_entity::IntoOneTimeExecutor<'_>,
        account_set_id: AccountSetId,
        args: es_entity::PaginatedQueryArgs<AccountSetMemberByExternalIdCursor>,
    ) -> Result<
        es_entity::PaginatedQueryRet<
            AccountSetMemberByExternalId,
            AccountSetMemberByExternalIdCursor,
        >,
        AccountSetError,
    > {
        let es_entity::PaginatedQueryArgs { first, after } = args;
        let (member_id, external_id) = if let Some(after) = after {
            (Some(after.id), after.external_id)
        } else {
            (None, None)
        };

        let id = match member_id {
            Some(member_id) => match member_id {
                AccountSetMemberId::Account(id) => Some(id),
                AccountSetMemberId::AccountSet(id) => Some(id.into()),
            },
            None => None,
        };

        let rows = op
            .into_executor()
            .fetch_all(sqlx::query!(
                r#"
            WITH member_accounts AS (
              SELECT
                member_account_id AS member_id,
                member_account_id,
                NULL::uuid AS member_account_set_id,
                a.external_id
              FROM cala_account_set_member_accounts m
              LEFT JOIN cala_accounts a ON m.member_account_id = a.id
              WHERE
                transitive IS FALSE
                AND m.account_set_id = $4
                AND (
                  ($3::varchar IS NULL) OR
                  (a.external_id IS NULL AND $3::varchar IS NOT NULL) OR
                  (a.external_id > $3::varchar) OR
                  (a.external_id = $3::varchar AND member_account_id > $2)
                )
              ORDER BY a.external_id ASC NULLS LAST, member_account_id ASC
              LIMIT $1
            ), member_sets AS (
              SELECT
                member_account_set_id AS member_id,
                NULL::uuid AS member_account_id,
                member_account_set_id,
                s.external_id
              FROM cala_account_set_member_account_sets m
              LEFT JOIN cala_account_sets s ON m.member_account_set_id = s.id
              WHERE
                m.account_set_id = $4
                AND (
                  ($3::varchar IS NULL) OR
                  (s.external_id IS NULL AND $3::varchar IS NOT NULL) OR
                  (s.external_id > $3::varchar) OR
                  (s.external_id = $3::varchar AND member_account_set_id > $2)
                )
              ORDER BY s.external_id ASC NULLS LAST, member_account_set_id ASC
              LIMIT $1
            ), all_members AS (
              SELECT * FROM member_accounts
              UNION ALL
              SELECT * FROM member_sets
            )
            SELECT * FROM all_members
            ORDER BY external_id ASC NULLS LAST, member_id ASC
            LIMIT $1
        "#,
                (first + 1) as i64,
                id.map(uuid::Uuid::from),
                external_id,
                uuid::Uuid::from(account_set_id),
            ))
            .await?;

        let has_next_page = rows.len() > first;
        let mut end_cursor = None;
        if let Some(last) = rows.last() {
            let id = last
                .member_account_id
                .map(|account_id| AccountSetMemberId::Account(account_id.into()))
                .or_else(|| {
                    last.member_account_set_id
                        .map(|account_set_id| AccountSetMemberId::AccountSet(account_set_id.into()))
                });
            end_cursor = Some(AccountSetMemberByExternalIdCursor {
                id: id.expect("member_id not set"),
                external_id: last.external_id.clone(),
            });
        }

        let account_set_members = rows
            .into_iter()
            .take(first)
            .map(
                |row| match (row.member_account_id, row.member_account_set_id) {
                    (Some(member_account_id), _) => AccountSetMemberByExternalId {
                        id: AccountSetMemberId::Account(AccountId::from(member_account_id)),
                        external_id: row.external_id,
                    },
                    (_, Some(member_account_set_id)) => AccountSetMemberByExternalId {
                        id: AccountSetMemberId::AccountSet(AccountSetId::from(
                            member_account_set_id,
                        )),
                        external_id: row.external_id,
                    },
                    _ => unreachable!(),
                },
            )
            .collect::<Vec<AccountSetMemberByExternalId>>();

        Ok(es_entity::PaginatedQueryRet {
            entities: account_set_members,
            has_next_page,
            end_cursor,
        })
    }

    #[instrument(
        name = "account_set.add_member_account_and_return_parents",
        skip_all,
        err(level = "warn")
    )]
    pub async fn add_member_account_and_return_parents(
        &self,
        db: &mut impl es_entity::AtomicOperation,
        account_set_id: AccountSetId,
        account_id: AccountId,
    ) -> Result<(DateTime<Utc>, Vec<AccountSetId>), AccountSetError> {
        lock_membership_scope(db, &[account_set_id], Some(account_id), false).await?;
        let rows = sqlx::query!(r#"
          WITH RECURSIVE parents AS (
            SELECT m.member_account_set_id, m.account_set_id
            FROM cala_account_set_member_account_sets m
            JOIN cala_account_sets s
            ON s.id = m.account_set_id
            WHERE m.member_account_set_id = $1

            UNION ALL
            SELECT p.member_account_set_id, m.account_set_id
            FROM parents p
            JOIN cala_account_set_member_account_sets m
                ON p.account_set_id = m.member_account_set_id
          ),
          non_transitive_insert AS (
            INSERT INTO cala_account_set_member_accounts (account_set_id, member_account_id)
            VALUES ($1, $2)
          ),
          transitive_insert AS (
            INSERT INTO cala_account_set_member_accounts (account_set_id, member_account_id, transitive)
            SELECT p.account_set_id, $2, TRUE
            FROM parents p
          )
          SELECT account_set_id, NULL AS now
          FROM parents
          UNION ALL
          SELECT NULL AS account_set_id, NOW() AS now
          "#,
            account_set_id as AccountSetId,
            account_id as AccountId,
        )
        .fetch_all(db.as_executor())
        .await?;
        let mut time = None;
        let ret = rows
            .into_iter()
            .filter_map(|row| {
                if let Some(t) = row.now {
                    time = Some(t);
                    None
                } else {
                    Some(AccountSetId::from(
                        row.account_set_id.expect("account_set_id not set"),
                    ))
                }
            })
            .collect();

        self.publisher
            .publish_all(
                db,
                std::iter::once(crate::outbox::OutboxEventPayload::AccountSetMemberCreated {
                    account_set_id,
                    member_id: crate::account_set::AccountSetMemberId::Account(account_id),
                }),
            )
            .await?;

        Ok((time.expect("time not set"), ret))
    }

    #[instrument(
        name = "account_set.remove_member_account_and_return_parents",
        skip_all,
        err(level = "warn")
    )]
    pub async fn remove_member_account_and_return_parents(
        &self,
        db: &mut impl es_entity::AtomicOperation,
        account_set_id: AccountSetId,
        account_id: AccountId,
    ) -> Result<(DateTime<Utc>, Vec<AccountSetId>), AccountSetError> {
        lock_membership_scope(db, &[account_set_id], Some(account_id), false).await?;
        let rows = sqlx::query!(
            r#"
          WITH RECURSIVE parents AS (
            SELECT m.member_account_set_id, m.account_set_id
            FROM cala_account_set_member_account_sets m
            JOIN cala_account_sets s
            ON s.id = m.account_set_id
            WHERE m.member_account_set_id = $1

            UNION ALL
            SELECT p.member_account_set_id, m.account_set_id
            FROM parents p
            JOIN cala_account_set_member_account_sets m
                ON p.account_set_id = m.member_account_set_id
          ),
          deletions as (
            DELETE FROM cala_account_set_member_accounts
            WHERE account_set_id IN (SELECT account_set_id FROM parents UNION SELECT $1)
            AND member_account_id = $2
          )
          SELECT account_set_id, NULL AS now
          FROM parents
          UNION ALL
          SELECT NULL AS account_set_id, NOW() AS now
          "#,
            account_set_id as AccountSetId,
            account_id as AccountId,
        )
        .fetch_all(db.as_executor())
        .await?;
        let mut time = None;
        let ret = rows
            .into_iter()
            .filter_map(|row| {
                if let Some(t) = row.now {
                    time = Some(t);
                    None
                } else {
                    Some(AccountSetId::from(
                        row.account_set_id.expect("account_set_id not set"),
                    ))
                }
            })
            .collect();

        self.publisher
            .publish_all(
                db,
                std::iter::once(crate::outbox::OutboxEventPayload::AccountSetMemberRemoved {
                    account_set_id,
                    member_id: crate::account_set::AccountSetMemberId::Account(account_id),
                }),
            )
            .await?;

        Ok((time.expect("time not set"), ret))
    }

    #[instrument(
        name = "account_set.add_member_set_and_return_parents",
        skip_all,
        err(level = "warn")
    )]
    pub async fn add_member_set_and_return_parents(
        &self,
        db: &mut impl es_entity::AtomicOperation,
        account_set_id: AccountSetId,
        member_account_set_id: AccountSetId,
    ) -> Result<(DateTime<Utc>, Vec<AccountSetId>), AccountSetError> {
        lock_membership_scope(db, &[account_set_id, member_account_set_id], None, true).await?;
        let rows = sqlx::query!(r#"
          WITH RECURSIVE parents AS (
            SELECT m.member_account_set_id, m.account_set_id
            FROM cala_account_set_member_account_sets m
            JOIN cala_account_sets s
            ON s.id = m.account_set_id
            WHERE m.member_account_set_id = $1

            UNION ALL
            SELECT p.member_account_set_id, m.account_set_id
            FROM parents p
            JOIN cala_account_set_member_account_sets m
                ON p.account_set_id = m.member_account_set_id
          ),
          set_insert AS (
            INSERT INTO cala_account_set_member_account_sets (account_set_id, member_account_set_id)
            VALUES ($1, $2)
          ),
          new_members AS (
            INSERT INTO cala_account_set_member_accounts (account_set_id, member_account_id, transitive)
            SELECT $1, m.member_account_id, TRUE
            FROM cala_account_set_member_accounts m
            WHERE m.account_set_id = $2
            RETURNING member_account_id
          ),
          transitive_inserts AS (
            INSERT INTO cala_account_set_member_accounts (account_set_id, member_account_id, transitive)
            SELECT p.account_set_id, n.member_account_id, TRUE
            FROM parents p
            CROSS JOIN new_members n
          )
          SELECT account_set_id, NULL AS now
          FROM parents
          UNION ALL
          SELECT NULL AS account_set_id, NOW() AS now
          "#,
            account_set_id as AccountSetId,
            member_account_set_id as AccountSetId,
        )
        .fetch_all(db.as_executor())
        .await?;
        let mut time = None;
        let ret = rows
            .into_iter()
            .filter_map(|row| {
                if let Some(t) = row.now {
                    time = Some(t);
                    None
                } else {
                    Some(AccountSetId::from(
                        row.account_set_id.expect("account_set_id not set"),
                    ))
                }
            })
            .collect();

        self.publisher
            .publish_all(
                db,
                std::iter::once(crate::outbox::OutboxEventPayload::AccountSetMemberCreated {
                    account_set_id,
                    member_id: crate::account_set::AccountSetMemberId::AccountSet(
                        member_account_set_id,
                    ),
                }),
            )
            .await?;

        Ok((time.expect("time not set"), ret))
    }

    #[instrument(
        name = "account_set.remove_member_set_and_return_parents",
        skip_all,
        err(level = "warn")
    )]
    pub async fn remove_member_set_and_return_parents(
        &self,
        db: &mut impl es_entity::AtomicOperation,
        account_set_id: AccountSetId,
        member_account_set_id: AccountSetId,
    ) -> Result<(DateTime<Utc>, Vec<AccountSetId>), AccountSetError> {
        lock_membership_scope(db, &[account_set_id, member_account_set_id], None, true).await?;
        let rows = sqlx::query!(
            r#"
          WITH RECURSIVE parents AS (
            SELECT m.member_account_set_id, m.account_set_id
            FROM cala_account_set_member_account_sets m
            JOIN cala_account_sets s
            ON s.id = m.account_set_id
            WHERE m.member_account_set_id = $1

            UNION ALL
            SELECT p.member_account_set_id, m.account_set_id
            FROM parents p
            JOIN cala_account_set_member_account_sets m
                ON p.account_set_id = m.member_account_set_id
          ),
          member_accounts_deletion AS (
            DELETE FROM cala_account_set_member_accounts
            WHERE account_set_id IN (SELECT account_set_id FROM parents UNION SELECT $1)
            AND member_account_id IN (SELECT member_account_id FROM cala_account_set_member_accounts
                                      WHERE account_set_id = $2)
          ),
          member_account_set_deletion AS (
            DELETE FROM cala_account_set_member_account_sets
            WHERE account_set_id IN (SELECT account_set_id FROM parents UNION SELECT $1)
            AND member_account_set_id = $2
          )
          SELECT account_set_id, NULL AS now
          FROM parents
          UNION ALL
          SELECT NULL AS account_set_id, NOW() AS now
          "#,
            account_set_id as AccountSetId,
            member_account_set_id as AccountSetId,
        )
        .fetch_all(db.as_executor())
        .await?;
        let mut time = None;
        let ret = rows
            .into_iter()
            .filter_map(|row| {
                if let Some(t) = row.now {
                    time = Some(t);
                    None
                } else {
                    Some(AccountSetId::from(
                        row.account_set_id.expect("account_set_id not set"),
                    ))
                }
            })
            .collect();

        self.publisher
            .publish_all(
                db,
                std::iter::once(crate::outbox::OutboxEventPayload::AccountSetMemberRemoved {
                    account_set_id,
                    member_id: crate::account_set::AccountSetMemberId::AccountSet(
                        member_account_set_id,
                    ),
                }),
            )
            .await?;

        Ok((time.expect("time not set"), ret))
    }

    pub async fn find_where_account_is_member(
        &self,
        account_id: AccountId,
        query: es_entity::PaginatedQueryArgs<AccountSetByNameCursor>,
    ) -> Result<es_entity::PaginatedQueryRet<AccountSet, AccountSetByNameCursor>, AccountSetError>
    {
        self.find_where_account_is_member_in_op(&self.pool, account_id, query)
            .await
    }

    pub async fn find_where_account_is_member_in_op(
        &self,
        op: impl es_entity::IntoOneTimeExecutor<'_>,
        account_id: AccountId,
        query: es_entity::PaginatedQueryArgs<AccountSetByNameCursor>,
    ) -> Result<es_entity::PaginatedQueryRet<AccountSet, AccountSetByNameCursor>, AccountSetError>
    {
        let (entities, has_next_page) = es_entity::es_query!(
            tbl_prefix = "cala",
            r#"SELECT a.id, a.name, a.created_at
              FROM cala_account_sets a
              JOIN cala_account_set_member_accounts asm
              ON asm.account_set_id = a.id
              WHERE asm.member_account_id = $1 AND transitive IS FALSE
              AND ((a.name, a.id) > ($3, $2) OR ($3 IS NULL AND $2 IS NULL))
              ORDER BY a.name, a.id
              LIMIT $4"#,
            account_id as AccountId,
            query.after.as_ref().map(|c| c.id) as Option<AccountSetId>,
            query.after.map(|c| c.name),
            query.first as i64 + 1
        )
        .fetch_n(op, query.first)
        .await?;

        let mut end_cursor = None;
        if let Some(last) = entities.last() {
            end_cursor = Some(AccountSetByNameCursor {
                id: last.values().id,
                name: last.values().name.clone(),
            });
        }
        Ok(es_entity::PaginatedQueryRet {
            entities,
            has_next_page,
            end_cursor,
        })
    }

    pub async fn find_where_account_set_is_member(
        &self,
        account_set_id: AccountSetId,
        query: es_entity::PaginatedQueryArgs<AccountSetByNameCursor>,
    ) -> Result<es_entity::PaginatedQueryRet<AccountSet, AccountSetByNameCursor>, AccountSetError>
    {
        self.find_where_account_set_is_member_in_op(&self.pool, account_set_id, query)
            .await
    }

    pub async fn find_where_account_set_is_member_in_op(
        &self,
        op: impl es_entity::IntoOneTimeExecutor<'_>,
        account_set_id: AccountSetId,
        query: es_entity::PaginatedQueryArgs<AccountSetByNameCursor>,
    ) -> Result<es_entity::PaginatedQueryRet<AccountSet, AccountSetByNameCursor>, AccountSetError>
    {
        let (entities, has_next_page) = es_entity::es_query!(
            tbl_prefix = "cala",
            r#"SELECT a.id, a.name, a.created_at
               FROM cala_account_sets a
               JOIN cala_account_set_member_account_sets asm
               ON asm.account_set_id = a.id
               WHERE asm.member_account_set_id = $1
               AND ((a.name, a.id) > ($3, $2) OR ($3 IS NULL AND $2 IS NULL))
               ORDER BY a.name, a.id
               LIMIT $4"#,
            account_set_id as AccountSetId,
            query.after.as_ref().map(|c| c.id) as Option<AccountSetId>,
            query.after.map(|c| c.name),
            query.first as i64 + 1
        )
        .fetch_n(op, query.first)
        .await?;
        let mut end_cursor = None;
        if let Some(last) = entities.last() {
            end_cursor = Some(AccountSetByNameCursor {
                id: last.values().id,
                name: last.values().name.clone(),
            });
        }
        Ok(es_entity::PaginatedQueryRet {
            entities,
            has_next_page,
            end_cursor,
        })
    }

    #[instrument(
        name = "account_set.fetch_mappings_in_op",
        skip_all,
        err(level = "warn")
    )]
    pub async fn fetch_mappings_in_op(
        &self,
        op: impl es_entity::IntoOneTimeExecutor<'_>,
        journal_id: JournalId,
        account_ids: &[AccountId],
    ) -> Result<HashMap<AccountId, Vec<AccountSetId>>, AccountSetError> {
        let rows = op.into_executor().fetch_all(sqlx::query!(
            r#"
          SELECT m.account_set_id AS "set_id!: AccountSetId", m.member_account_id AS "account_id!: AccountId"
          FROM cala_account_set_member_accounts m
          JOIN cala_account_sets s
          ON m.account_set_id = s.id AND s.journal_id = $1
          WHERE m.member_account_id = ANY($2)
          "#,
            journal_id as JournalId,
            account_ids as &[AccountId]
        ))
        .await?;
        let mut mappings = HashMap::new();
        for row in rows {
            mappings
                .entry(row.account_id)
                .or_insert_with(Vec::new)
                .push(row.set_id);
        }
        Ok(mappings)
    }

    pub async fn list_eventually_consistent_ids(
        &self,
        args: es_entity::PaginatedQueryArgs<AccountSetByIdCursor>,
    ) -> Result<es_entity::PaginatedQueryRet<AccountSetId, AccountSetByIdCursor>, AccountSetError>
    {
        self.list_eventually_consistent_ids_in_op(&self.pool, args)
            .await
    }

    // Uses raw `sqlx::query!` (rather than `es_query!`) because it only needs
    // account-set ids — not fully hydrated `AccountSet` entities — which keeps
    // periodic reconciliation jobs cheap as the number of EC account sets grows.
    #[instrument(
        name = "account_set.list_eventually_consistent_ids_in_op",
        skip_all,
        err(level = "warn")
    )]
    pub async fn list_eventually_consistent_ids_in_op(
        &self,
        op: impl es_entity::IntoOneTimeExecutor<'_>,
        args: es_entity::PaginatedQueryArgs<AccountSetByIdCursor>,
    ) -> Result<es_entity::PaginatedQueryRet<AccountSetId, AccountSetByIdCursor>, AccountSetError>
    {
        let es_entity::PaginatedQueryArgs { first, after } = args;

        let rows = op
            .into_executor()
            .fetch_all(sqlx::query!(
                r#"
            SELECT s.id AS "id!: AccountSetId"
            FROM cala_account_sets s
            JOIN cala_accounts a ON s.id = a.id
            WHERE a.eventually_consistent = TRUE
              AND ($2::uuid IS NULL OR s.id > $2)
            ORDER BY s.id ASC
            LIMIT $1
            "#,
                (first + 1) as i64,
                after.map(|c| uuid::Uuid::from(c.id)),
            ))
            .await?;

        let has_next_page = rows.len() > first;
        let entities: Vec<AccountSetId> = rows.into_iter().take(first).map(|r| r.id).collect();
        let end_cursor = entities.last().map(|id| AccountSetByIdCursor { id: *id });

        Ok(es_entity::PaginatedQueryRet {
            entities,
            has_next_page,
            end_cursor,
        })
    }

    /// Walk the descendant account sets of `account_set_ids` transitively
    /// and return the ones whose underlying account is
    /// `eventually_consistent = TRUE`. Non-EC descendants are filtered
    /// out at the SQL level so callers (the recalc deep walk) don't try
    /// to recalc them.
    #[instrument(
        name = "account_set.find_all_ec_descendant_set_ids",
        skip_all,
        err(level = "warn")
    )]
    pub async fn find_all_ec_descendant_set_ids(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        account_set_ids: &[AccountSetId],
    ) -> Result<Vec<AccountSetId>, AccountSetError> {
        let rows = sqlx::query!(
            r#"
            WITH RECURSIVE descendants AS (
                SELECT member_account_set_id AS id
                FROM cala_account_set_member_account_sets
                WHERE account_set_id = ANY($1)
                UNION
                SELECT m.member_account_set_id
                FROM cala_account_set_member_account_sets m
                JOIN descendants d ON d.id = m.account_set_id
            )
            SELECT d.id AS "id!: AccountSetId"
            FROM descendants d
            JOIN cala_accounts a ON a.id = d.id
            WHERE a.eventually_consistent = TRUE
            "#,
            account_set_ids as &[AccountSetId],
        )
        .fetch_all(op.as_executor())
        .await?;

        Ok(rows.into_iter().map(|r| r.id).collect())
    }

    async fn publish(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        entity: &AccountSet,
        new_events: es_entity::LastPersisted<'_, AccountSetEvent>,
    ) -> Result<(), sqlx::Error> {
        self.publisher
            .publish_entity_events(op, entity, new_events)
            .await?;
        Ok(())
    }
}
