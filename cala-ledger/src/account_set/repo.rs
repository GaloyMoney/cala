use es_entity::*;
use sqlx::PgPool;
use tracing::instrument;


use crate::{
    outbox::OutboxPublisher,
    primitives::{AccountId, JournalId},
};

use super::{entity::*, error::*};

/// Coarse advisory lock guarding the account-set membership graph
/// (`cala_account_set_member_accounts` and
/// `cala_account_set_member_account_sets`).
///
/// Membership maintenance is a read-then-write over the whole ancestor
/// chain: each mutation walks the set-to-set edges (the recursive
/// `parents` CTE) and then writes the transitive-closure rows the walk
/// justifies. Two concurrent mutations that each miss the other's
/// uncommitted writes would leave the closure table inconsistent
/// (write-skew), so the walk's snapshot has to stay valid until the
/// writes commit — which is why every lock here is transaction-scoped
/// and held to commit; releasing earlier would reopen the race.
///
/// Lock protocol:
///
/// - Set-structure mutations (`add_member_set` / `remove_member_set`)
///   take this lock EXCLUSIVE. They mutate the edges that every walk
///   reads, and read the member rows that account-member mutations
///   write, so they must exclude everything.
/// - Account-member mutations (`add_member_account` /
///   `remove_member_account`) take this lock SHARED plus an EXCLUSIVE
///   per-member lock (`MEMBER_LOCK_CLASS`, keyed on the member account
///   id). Shared-vs-exclusive fences them against structure mutations,
///   while account-member mutations for *different* members run
///   concurrently — their closure writes are disjoint rows. The
///   per-member lock serializes mutations touching the *same* member
///   (e.g. an add and a remove in overlapping hierarchies), whose
///   interleaved inserts/deletes on shared ancestors would otherwise
///   tear the closure.
///
/// Ordering: the coarse lock is always acquired before the per-member
/// lock. An operation must never wait on the coarse lock while holding
/// a per-member lock — under PostgreSQL's FIFO lock queueing that can
/// form a wait cycle with a queued exclusive (structure) waiter.
const ADDVISORY_LOCK_ID: i64 = 123456;

/// `classid` namespace for the per-member advisory locks (2-arg form),
/// keyed on `hashtext(<member account id>)`. Must stay disjoint from
/// `EC_SET_LOCK_CLASS` (= 1) used by balance locking.
const MEMBER_LOCK_CLASS: i32 = 2;

/// Takes the account-member half of the membership lock protocol (see
/// [`ADDVISORY_LOCK_ID`]): SHARED coarse lock, then EXCLUSIVE
/// per-member lock. Two statements so the acquisition order is
/// guaranteed.
async fn lock_for_account_member_op(
    db: &mut impl es_entity::AtomicOperation,
    account_id: AccountId,
) -> Result<(), AccountSetError> {
    sqlx::query!("SELECT pg_advisory_xact_lock_shared($1)", ADDVISORY_LOCK_ID)
        .execute(db.as_executor())
        .await?;
    sqlx::query!(
        "SELECT pg_advisory_xact_lock($1, hashtext($2))",
        MEMBER_LOCK_CLASS,
        account_id.to_string(),
    )
    .execute(db.as_executor())
    .await?;
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
        level = "debug",
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
        level = "debug",
        name = "account_set.add_member_account",
        skip_all,
        err(level = "warn")
    )]
    pub async fn add_member_account(
        &self,
        db: &mut impl es_entity::AtomicOperation,
        account_set_id: AccountSetId,
        account_id: AccountId,
    ) -> Result<(), AccountSetError> {
        lock_for_account_member_op(db, account_id).await?;
        // Direct membership only; ancestor (transitive) rows are
        // materialized asynchronously by the fill job, and postings walk
        // the live hierarchy while `transitive_complete` is FALSE.
        sqlx::query!(r#"
          INSERT INTO cala_account_set_member_accounts (account_set_id, member_account_id)
          VALUES ($1, $2)
          "#,
            account_set_id as AccountSetId,
            account_id as AccountId,
        )
        .execute(db.as_executor())
        .await?;

        self.publisher
            .publish_all(
                db,
                std::iter::once(crate::outbox::OutboxEventPayload::AccountSetMemberCreated {
                    account_set_id,
                    member_id: crate::account_set::AccountSetMemberId::Account(account_id),
                }),
            )
            .await?;

        Ok(())
    }

    /// Batch variant of [`add_member_account`]: attaches every
    /// `(account_set_id, account_id)` pair in one statement. Only direct
    /// rows are written; ancestor rows are materialized asynchronously by
    /// the fill job. Callers creating many accounts (e.g. a
    /// chart-of-accounts expansion per business entity) should prefer
    /// this over looping `add_member_account`.
    ///
    /// Lock protocol matches the single-pair path (SHARED coarse lock,
    /// then EXCLUSIVE per-member locks, in that order) with all member
    /// locks taken in one id-ordered statement.
    #[instrument(
        level = "debug",
        name = "account_set.add_member_accounts",
        skip_all,
        fields(count = members.len()),
        err(level = "warn")
    )]
    pub async fn add_member_accounts(
        &self,
        db: &mut impl es_entity::AtomicOperation,
        members: &[(AccountSetId, AccountId)],
    ) -> Result<(), AccountSetError> {
        if members.is_empty() {
            return Ok(());
        }
        let account_set_ids: Vec<AccountSetId> = members.iter().map(|(s, _)| *s).collect();
        let account_ids: Vec<AccountId> = members.iter().map(|(_, a)| *a).collect();

        // Sort and dedup the lock ids in Rust so the per-member locks
        // are always acquired in canonical id order, matching the
        // single-pair path. A SQL ORDER BY is not a reliable
        // substitute: the planner is free to evaluate the lock
        // projection before any sort node. (`account_ids` itself must
        // stay pair-aligned with `account_set_ids` for the insert
        // below, hence the separate vector.)
        let mut lock_ids = account_ids.clone();
        lock_ids.sort();
        lock_ids.dedup();

        sqlx::query!("SELECT pg_advisory_xact_lock_shared($1)", ADDVISORY_LOCK_ID)
            .execute(db.as_executor())
            .await?;
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

        sqlx::query!(
            r#"
          INSERT INTO cala_account_set_member_accounts (account_set_id, member_account_id)
          SELECT * FROM UNNEST($1::uuid[], $2::uuid[]) AS v(account_set_id, account_id)
          "#,
            &account_set_ids as &[AccountSetId],
            &account_ids as &[AccountId],
        )
        .execute(db.as_executor())
        .await?;

        self.publisher
            .publish_all(
                db,
                members.iter().map(|(account_set_id, account_id)| {
                    crate::outbox::OutboxEventPayload::AccountSetMemberCreated {
                        account_set_id: *account_set_id,
                        member_id: crate::account_set::AccountSetMemberId::Account(*account_id),
                    }
                }),
            )
            .await?;

        Ok(())
    }

    #[instrument(
        level = "debug",
        name = "account_set.remove_member_account",
        skip_all,
        err(level = "warn")
    )]
    pub async fn remove_member_account(
        &self,
        db: &mut impl es_entity::AtomicOperation,
        account_set_id: AccountSetId,
        account_id: AccountId,
    ) -> Result<(), AccountSetError> {
        lock_for_account_member_op(db, account_id).await?;
        sqlx::query!(
            r#"
          WITH RECURSIVE parents AS (
            SELECT m.member_account_set_id, m.account_set_id
            FROM cala_account_set_member_account_sets m
            WHERE m.member_account_set_id = $1

            UNION ALL
            SELECT p.member_account_set_id, m.account_set_id
            FROM parents p
            JOIN cala_account_set_member_account_sets m
                ON p.account_set_id = m.member_account_set_id
          )
          DELETE FROM cala_account_set_member_accounts
          WHERE account_set_id IN (SELECT account_set_id FROM parents UNION SELECT $1)
          AND member_account_id = $2
          "#,
            account_set_id as AccountSetId,
            account_id as AccountId,
        )
        .execute(db.as_executor())
        .await?;

        self.publisher
            .publish_all(
                db,
                std::iter::once(crate::outbox::OutboxEventPayload::AccountSetMemberRemoved {
                    account_set_id,
                    member_id: crate::account_set::AccountSetMemberId::Account(account_id),
                }),
            )
            .await?;

        Ok(())
    }

    #[instrument(
        level = "debug",
        name = "account_set.add_member_set",
        skip_all,
        err(level = "warn")
    )]
    pub async fn add_member_set(
        &self,
        db: &mut impl es_entity::AtomicOperation,
        account_set_id: AccountSetId,
        member_account_set_id: AccountSetId,
    ) -> Result<(), AccountSetError> {
        // Structure mutation: EXCLUSIVE coarse lock (see ADDVISORY_LOCK_ID).
        sqlx::query!("SELECT pg_advisory_xact_lock($1)", ADDVISORY_LOCK_ID)
            .execute(db.as_executor())
            .await?;
        sqlx::query!(r#"
          WITH RECURSIVE descendants AS (
            SELECT $2::uuid AS account_set_id
            UNION
            SELECT m.member_account_set_id
            FROM descendants d
            JOIN cala_account_set_member_account_sets m
                ON d.account_set_id = m.account_set_id
          ),
          set_insert AS (
            INSERT INTO cala_account_set_member_account_sets (account_set_id, member_account_set_id)
            VALUES ($1, $2)
          )
          -- Every direct membership in the member set's closure gains new
          -- ancestors from this edge; the fill job re-materializes them.
          UPDATE cala_account_set_member_accounts
          SET transitive_complete = FALSE
          WHERE transitive IS FALSE
            AND transitive_complete IS TRUE
            AND account_set_id IN (SELECT account_set_id FROM descendants)
          "#,
            account_set_id as AccountSetId,
            member_account_set_id as AccountSetId,
        )
        .execute(db.as_executor())
        .await?;

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

        Ok(())
    }

    #[instrument(
        level = "debug",
        name = "account_set.remove_member_set",
        skip_all,
        err(level = "warn")
    )]
    pub async fn remove_member_set(
        &self,
        db: &mut impl es_entity::AtomicOperation,
        account_set_id: AccountSetId,
        member_account_set_id: AccountSetId,
    ) -> Result<(), AccountSetError> {
        // Structure mutation: EXCLUSIVE coarse lock (see ADDVISORY_LOCK_ID).
        sqlx::query!("SELECT pg_advisory_xact_lock($1)", ADDVISORY_LOCK_ID)
            .execute(db.as_executor())
            .await?;
        sqlx::query!(
            r#"
          WITH RECURSIVE parents AS (
            SELECT m.member_account_set_id, m.account_set_id
            FROM cala_account_set_member_account_sets m
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
          )
          DELETE FROM cala_account_set_member_account_sets
          WHERE account_set_id IN (SELECT account_set_id FROM parents UNION SELECT $1)
          AND member_account_set_id = $2
          "#,
            account_set_id as AccountSetId,
            member_account_set_id as AccountSetId,
        )
        .execute(db.as_executor())
        .await?;

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

        Ok(())
    }

    /// Guard replacing the collision detection that used to fall out of
    /// the unique constraint on materialized transitive rows: with async
    /// fill those rows may not exist yet, so check membership against the
    /// live hierarchy — an account may not attach under a set if it is
    /// already a (direct) member of that set, of any of its ancestors, or
    /// of any set in those ancestors' subtrees (e.g. a sibling branch).
    #[instrument(
        level = "debug",
        name = "account_set.assert_members_absent_in_op",
        skip_all,
        fields(count = members.len()),
        err(level = "warn")
    )]
    pub async fn assert_members_absent_in_op(
        &self,
        db: &mut impl es_entity::AtomicOperation,
        members: &[(AccountSetId, AccountId)],
    ) -> Result<(), AccountSetError> {
        let account_set_ids: Vec<AccountSetId> = members.iter().map(|(s, _)| *s).collect();
        let account_ids: Vec<AccountId> = members.iter().map(|(_, a)| *a).collect();
        let found = sqlx::query!(
            r#"
          WITH RECURSIVE input_pairs AS (
            SELECT * FROM UNNEST($1::uuid[], $2::uuid[]) AS v(account_set_id, account_id)
          ),
          up AS (
            SELECT i.account_id, m.member_account_set_id, m.account_set_id
            FROM input_pairs i
            JOIN cala_account_set_member_account_sets m
                ON m.member_account_set_id = i.account_set_id
            UNION ALL
            SELECT u.account_id, u.member_account_set_id, m.account_set_id
            FROM up u
            JOIN cala_account_set_member_account_sets m
                ON u.account_set_id = m.member_account_set_id
          ),
          targets AS (
            SELECT account_set_id, account_id FROM input_pairs
            UNION ALL
            SELECT account_set_id, account_id FROM up
          ),
          down AS (
            SELECT t.account_set_id, t.account_id FROM targets t
            UNION
            SELECT m.member_account_set_id, d.account_id
            FROM down d
            JOIN cala_account_set_member_account_sets m
                ON d.account_set_id = m.account_set_id
          )
          SELECT 1 AS found FROM cala_account_set_member_accounts ma
          JOIN down d ON d.account_set_id = ma.account_set_id
                     AND d.account_id = ma.member_account_id
          WHERE ma.transitive IS FALSE
          LIMIT 1
          "#,
            &account_set_ids as &[AccountSetId],
            &account_ids as &[AccountId],
        )
        .fetch_optional(db.as_executor())
        .await?;
        if found.is_some() {
            return Err(AccountSetError::MemberAlreadyAdded);
        }
        Ok(())
    }

    /// Set-edge variant of [`Self::assert_members_absent_in_op`]: attaching
    /// a set must not double-count any of its descendant accounts under
    /// the target's ancestor chain.
    #[instrument(
        level = "debug",
        name = "account_set.assert_member_set_absent_in_op",
        skip_all,
        err(level = "warn")
    )]
    pub async fn assert_member_set_absent_in_op(
        &self,
        db: &mut impl es_entity::AtomicOperation,
        account_set_id: AccountSetId,
        member_account_set_id: AccountSetId,
    ) -> Result<(), AccountSetError> {
        let found = sqlx::query!(
            r#"
          WITH RECURSIVE up AS (
            SELECT $1::uuid AS account_set_id
            UNION
            SELECT m.account_set_id
            FROM up u
            JOIN cala_account_set_member_account_sets m
                ON u.account_set_id = m.member_account_set_id
          ),
          up_subtrees AS (
            SELECT account_set_id FROM up
            UNION
            SELECT m.member_account_set_id
            FROM up_subtrees us
            JOIN cala_account_set_member_account_sets m
                ON us.account_set_id = m.account_set_id
          ),
          new_descendants AS (
            SELECT $2::uuid AS account_set_id
            UNION
            SELECT m.member_account_set_id
            FROM new_descendants d
            JOIN cala_account_set_member_account_sets m
                ON d.account_set_id = m.account_set_id
          )
          SELECT 1 AS found
          FROM cala_account_set_member_accounts ma
          WHERE ma.transitive IS FALSE
            AND ma.account_set_id IN (SELECT account_set_id FROM up_subtrees)
            AND ma.member_account_id IN (
              SELECT member_account_id FROM cala_account_set_member_accounts
              WHERE transitive IS FALSE
                AND account_set_id IN (SELECT account_set_id FROM new_descendants)
            )
          LIMIT 1
          "#,
            account_set_id as AccountSetId,
            member_account_set_id as AccountSetId,
        )
        .fetch_optional(db.as_executor())
        .await?;
        if found.is_some() {
            return Err(AccountSetError::MemberAlreadyAdded);
        }
        Ok(())
    }

    /// Materialize pending transitive membership rows in bulk. Direct
    /// attaches (account and set) only write the direct row; this job
    /// building block fills the ancestor rows afterwards so the posting
    /// path's `fetch_mappings_in_op` can keep using the single indexed
    /// lookup. Intended to be called by a periodically scheduled job in
    /// the host application. Returns the number of memberships filled
    /// (direct account rows flagged complete + set edges marked filled).
    #[instrument(
        level = "debug",
        name = "account_set.fill_pending_transitive_memberships_in_op",
        skip_all,
        err(level = "warn")
    )]
    pub async fn fill_pending_transitive_memberships_in_op(
        &self,
        db: &mut impl es_entity::AtomicOperation,
        limit: i64,
    ) -> Result<usize, AccountSetError> {
        let mut filled = 0usize;

        // SHARED coarse lock (same protocol as attach): fences against
        // concurrent add_member_set, which takes the EXCLUSIVE side and
        // would otherwise interleave flag invalidation with our fills.
        sqlx::query!("SELECT pg_advisory_xact_lock_shared($1)", ADDVISORY_LOCK_ID)
            .execute(db.as_executor())
            .await?;

        // Phase 1: set edges whose descendant accounts haven't been copied
        // up the ancestor chain yet.
        let pending_edges = sqlx::query!(
            r#"
            SELECT account_set_id AS "account_set_id!: AccountSetId",
                   member_account_set_id AS "member_account_set_id!: AccountSetId"
            FROM cala_account_set_member_account_sets
            WHERE members_filled IS FALSE
            LIMIT $1
            "#,
            limit
        )
        .fetch_all(db.as_executor())
        .await?;

        for edge in &pending_edges {
            sqlx::query!(
                r#"
              WITH RECURSIVE descendants AS (
                SELECT $2::uuid AS account_set_id
                UNION
                SELECT m.member_account_set_id
                FROM descendants d
                JOIN cala_account_set_member_account_sets m
                    ON d.account_set_id = m.account_set_id
              ),
              member_accounts AS (
                SELECT DISTINCT member_account_id
                FROM cala_account_set_member_accounts
                WHERE account_set_id IN (SELECT account_set_id FROM descendants)
                  AND transitive IS FALSE
              ),
              locks AS (
                SELECT pg_advisory_xact_lock($3, hashtext(member_account_id::text))
                FROM member_accounts
                ORDER BY member_account_id
              ),
              ancestors AS (
                SELECT $1::uuid AS account_set_id
                UNION
                SELECT m.account_set_id
                FROM ancestors a
                JOIN cala_account_set_member_account_sets m
                    ON a.account_set_id = m.member_account_set_id
              ),
              ins AS (
                INSERT INTO cala_account_set_member_accounts (account_set_id, member_account_id, transitive)
                SELECT a.account_set_id, ma.member_account_id, TRUE
                FROM ancestors a
                CROSS JOIN member_accounts ma
                ON CONFLICT (account_set_id, member_account_id) DO NOTHING
              )
              UPDATE cala_account_set_member_account_sets
              SET members_filled = TRUE
              WHERE account_set_id = $1 AND member_account_set_id = $2
                AND (SELECT count(*) FROM locks) >= 0
              "#,
                edge.account_set_id as AccountSetId,
                edge.member_account_set_id as AccountSetId,
                MEMBER_LOCK_CLASS,
            )
            .execute(db.as_executor())
            .await?;
            filled += 1;
        }

        // Phase 2: direct account memberships awaiting ancestor rows.
        let pending = sqlx::query!(
            r#"
            SELECT account_set_id AS "account_set_id!: AccountSetId",
                   member_account_id AS "member_account_id!: AccountId"
            FROM cala_account_set_member_accounts
            WHERE transitive IS FALSE
              AND transitive_complete IS FALSE
            LIMIT $1
            "#,
            limit
        )
        .fetch_all(db.as_executor())
        .await?;

        if !pending.is_empty() {
            let account_set_ids: Vec<AccountSetId> =
                pending.iter().map(|r| r.account_set_id).collect();
            let account_ids: Vec<AccountId> = pending.iter().map(|r| r.member_account_id).collect();

            // Per-member EXCLUSIVE locks (id-ordered, same protocol as
            // add_member_accounts) so a concurrent remove_member_account
            // either deletes before us or waits and then also deletes the
            // rows we insert — no resurrected memberships.
            sqlx::query!(
                r#"
              SELECT pg_advisory_xact_lock($1, hashtext(v.account_id::text))
              FROM UNNEST($2::uuid[]) AS v(account_id)
              ORDER BY v.account_id
              "#,
                MEMBER_LOCK_CLASS,
                &account_ids as &[AccountId],
            )
            .execute(db.as_executor())
            .await?;

            let res = sqlx::query!(
                r#"
              WITH RECURSIVE batch AS (
                SELECT d.account_set_id, d.member_account_id AS account_id
                FROM cala_account_set_member_accounts d
                WHERE d.transitive IS FALSE
                  AND (d.account_set_id, d.member_account_id) IN (
                        SELECT * FROM UNNEST($1::uuid[], $2::uuid[]))
              ),
              parents AS (
                SELECT b.account_id, m.member_account_set_id, m.account_set_id
                FROM batch b
                JOIN cala_account_set_member_account_sets m
                    ON m.member_account_set_id = b.account_set_id
                UNION ALL
                SELECT p.account_id, p.member_account_set_id, m.account_set_id
                FROM parents p
                JOIN cala_account_set_member_account_sets m
                    ON p.account_set_id = m.member_account_set_id
              ),
              ins AS (
                INSERT INTO cala_account_set_member_accounts (account_set_id, member_account_id, transitive)
                SELECT p.account_set_id, p.account_id, TRUE
                FROM parents p
                ON CONFLICT (account_set_id, member_account_id) DO NOTHING
              )
              UPDATE cala_account_set_member_accounts m
              SET transitive_complete = TRUE
              FROM batch b
              WHERE m.account_set_id = b.account_set_id
                AND m.member_account_id = b.account_id
                AND m.transitive IS FALSE
              "#,
                &account_set_ids as &[AccountSetId],
                &account_ids as &[AccountId],
            )
            .execute(db.as_executor())
            .await?;
            filled += res.rows_affected() as usize;
        }

        Ok(filled)
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
        level = "debug",
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
        level = "debug",
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
