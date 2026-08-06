use es_entity::*;
use sqlx::PgPool;
use tracing::instrument;

use std::collections::HashMap;

use crate::{
    outbox::OutboxPublisher,
    primitives::{AccountId, JournalId},
};

use super::{entity::*, error::*};

/// Coarse advisory lock guarding the account-set membership graph
/// (`cala_account_set_member_accounts` and
/// `cala_account_set_member_account_sets`).
///
/// Membership is stored as **direct edges only** — ancestor sets are
/// resolved at read time by an upward recursive walk
/// (`fetch_mappings_in_op`, `fetch_ec_set_mappings`); there is no
/// materialized transitive closure. The graph still carries a
/// load-bearing invariant that every mutation must validate before
/// writing: **path uniqueness** — an account may be contained in any
/// given set via at most one membership path (double membership is
/// prohibited). The old closure enforced this incidentally through its
/// unique constraint; walk-only enforces it explicitly
/// (`assert_no_double_membership` and the set-level checks in
/// `add_member_set`). Each check is a read-then-write over the graph,
/// so it must be fenced against concurrent writers — which is why the
/// closure-era lock protocol survives walk-only unchanged:
///
/// - Set-structure mutations (`add_member_set` / `remove_member_set`)
///   take this lock EXCLUSIVE. They mutate the edges that every walk
///   reads, and read the member rows that account-member mutations
///   write, so they must exclude everything.
/// - Account-member mutations (`add_member_account(s)` /
///   `remove_member_account`) take this lock SHARED plus an EXCLUSIVE
///   per-member lock (`MEMBER_LOCK_CLASS`, keyed on the member account
///   id). Shared-vs-exclusive fences them against structure mutations,
///   while account-member mutations for *different* members run
///   concurrently — each validation involves only its own member's
///   paths. The per-member lock serializes mutations touching the
///   *same* member, whose interleaved check-then-write sequences could
///   otherwise commit a double membership.
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

/// Maximum depth (in set->set edges) of any root-to-leaf membership
/// chain. Enforced in `add_member_set`: rejecting edges past this bound
/// keeps the read-time ancestor walk cheap and terminating. Real
/// hierarchies are <=10 deep; 16 leaves headroom.
const MAX_MEMBERSHIP_DEPTH: i32 = 16;

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

    /// Takes the account-member half of the membership lock protocol (see
    /// [`ADDVISORY_LOCK_ID`]): SHARED coarse lock, then EXCLUSIVE
    /// per-member lock. Two statements so the acquisition order is
    /// guaranteed.
    async fn lock_for_account_member_op(
        &self,
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

    /// Rejects account-member additions that would give an account a second
    /// membership path to any set (double membership — see
    /// [`ADDVISORY_LOCK_ID`]). For every `(account_set_id, account_id)`
    /// pair it expands the containments the new direct edge would create
    /// (the target set plus all of its ancestors) and combines them with
    /// the account's existing containments; any set the same account would
    /// reach twice — via an existing path, via the new edge, or across two
    /// pairs of the same batch — is a violation. Surfaced as
    /// [`AccountSetError::MemberAlreadyAdded`], the same error the old
    /// closure's unique-constraint collision produced.
    ///
    /// Must run under the account-member lock protocol: the walk is a read
    /// over the set graph and the account's direct edges, and the insert
    /// that follows relies on its result staying valid until commit.
    async fn assert_no_double_membership(
        &self,
        db: &mut impl es_entity::AtomicOperation,
        account_set_ids: &[AccountSetId],
        account_ids: &[AccountId],
    ) -> Result<(), AccountSetError> {
        let row = sqlx::query!(
            r#"
            WITH RECURSIVE new_containments AS (
                SELECT v.account_id, v.account_set_id
                FROM UNNEST($1::uuid[], $2::uuid[]) AS v(account_set_id, account_id)

                UNION ALL
                SELECT nc.account_id, e.account_set_id
                FROM new_containments nc
                JOIN cala_account_set_member_account_sets e
                    ON e.member_account_set_id = nc.account_set_id
            ),
            existing_containments AS (
                SELECT m.member_account_id AS account_id, m.account_set_id
                FROM cala_account_set_member_accounts m
                WHERE m.member_account_id = ANY($2)

                UNION ALL
                SELECT ec.account_id, e.account_set_id
                FROM existing_containments ec
                JOIN cala_account_set_member_account_sets e
                    ON e.member_account_set_id = ec.account_set_id
            )
            SELECT EXISTS (
                SELECT 1 FROM (
                    SELECT account_id, account_set_id FROM new_containments
                    UNION ALL
                    SELECT account_id, account_set_id FROM existing_containments
                ) AS all_containments
                GROUP BY account_id, account_set_id
                HAVING COUNT(*) > 1
            ) AS "conflict!"
            "#,
            account_set_ids as &[AccountSetId],
            account_ids as &[AccountId],
        )
        .fetch_one(db.as_executor())
        .await?;
        if row.conflict {
            return Err(AccountSetError::MemberAlreadyAdded);
        }
        Ok(())
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
                account_set_id = $4
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
                m.account_set_id = $4
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
        self.lock_for_account_member_op(db, account_id).await?;
        self.assert_no_double_membership(db, &[account_set_id], &[account_id])
            .await?;
        // A single direct edge: ancestor sets are resolved by the
        // read-time walk, so there is no closure to materialize.
        sqlx::query!(
            r#"
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
    /// `(account_set_id, account_id)` pair in one direct-edge insert, with
    /// a single path-uniqueness validation walk covering all pairs (and
    /// their interactions with each other) instead of one per pair.
    /// Callers creating many accounts (e.g. a chart-of-accounts expansion
    /// per business entity) should prefer this over looping
    /// `add_member_account`.
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

        self.assert_no_double_membership(db, &account_set_ids, &account_ids)
            .await?;

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
        self.lock_for_account_member_op(db, account_id).await?;
        // Delete the single direct edge; there are no materialized
        // ancestor rows to scrub. The lock keeps same-member add/remove
        // interleavings serialized (see ADDVISORY_LOCK_ID).
        sqlx::query!(
            r#"
          DELETE FROM cala_account_set_member_accounts
          WHERE account_set_id = $1 AND member_account_id = $2
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
        // Held across the validation query and the insert below, so the
        // graph it validated cannot change before the new edge commits.
        sqlx::query!("SELECT pg_advisory_xact_lock($1)", ADDVISORY_LOCK_ID)
            .execute(db.as_executor())
            .await?;

        // A set can never be its own member; the recursive cycle check
        // below catches the transitive case.
        if account_set_id == member_account_set_id {
            return Err(AccountSetError::MembershipCycleDetected {
                account_set_id,
                member_account_set_id,
            });
        }

        // Validate the edge in a single round trip. The checks share
        // their walks, so they run as one statement over four CTEs:
        //
        // - `target_ancestors`: the target's ancestors with their
        //   distance from it, capped at MAX_MEMBERSHIP_DEPTH (complete,
        //   since the cap is enforced on every edge; the bound also
        //   keeps the walk terminating even on a corrupted graph).
        // - `member_subtree`: the member and its descendants with their
        //   distance below it, same cap.
        // - `subtree_reach`: every set any subtree member already
        //   reaches upward.
        // - `target_reach`: every set whose accounts already live under
        //   the target chain (down-walk from target + ancestors).
        //
        // Verdicts, in error-precedence order:
        //
        // 1. `cycle` (#800): the member is already an ancestor of the
        //    target — the edge would close a cycle and make membership
        //    resolution non-terminating.
        // 2. `set_conflict` (path uniqueness, set level): the member —
        //    or any set below it — already reaches the target or one of
        //    the target's ancestors, so the edge would give that set
        //    (and every account below it) a second path there. The walk
        //    covers the member's whole subtree, not just the member: a
        //    descendant can reach the target chain through an edge that
        //    bypasses the member entirely (`A⊃B`, `B⊃D`, `X⊃D` — then
        //    attaching X under A would double-contain D). Seeding the
        //    reach walk with the subtree itself also catches a duplicate
        //    direct edge before the unique constraint does; the subtree
        //    cannot legitimately intersect the target chain (that is the
        //    cycle case, rejected first).
        // 3. `account_conflict` (path uniqueness, account level): an
        //    account somewhere under the member set is already contained
        //    somewhere under the target chain — the edge would
        //    double-contain it.
        // 4. `depth`: the deepest root->leaf chain through the new edge
        //    ((edges above the target) + 1 + (edges below the member))
        //    must stay within MAX_MEMBERSHIP_DEPTH so the read-time
        //    ancestor walk stays cheap and bounded.
        let checks = sqlx::query!(
            r#"
          WITH RECURSIVE target_ancestors AS (
            SELECT e.account_set_id AS set_id, 1 AS depth
            FROM cala_account_set_member_account_sets e
            WHERE e.member_account_set_id = $1

            UNION
            SELECT e.account_set_id, a.depth + 1
            FROM target_ancestors a
            JOIN cala_account_set_member_account_sets e
                ON e.member_account_set_id = a.set_id
            WHERE a.depth < $3
          ),
          target_chain AS (
            SELECT $1::uuid AS set_id
            UNION
            SELECT set_id FROM target_ancestors
          ),
          member_subtree AS (
            SELECT $2::uuid AS set_id, 0 AS depth
            UNION
            SELECT e.member_account_set_id, s.depth + 1
            FROM member_subtree s
            JOIN cala_account_set_member_account_sets e
                ON e.account_set_id = s.set_id
            WHERE s.depth < $3
          ),
          subtree_reach AS (
            SELECT set_id FROM member_subtree
            UNION
            SELECT e.account_set_id
            FROM subtree_reach r
            JOIN cala_account_set_member_account_sets e
                ON e.member_account_set_id = r.set_id
          ),
          target_reach AS (
            SELECT set_id FROM target_chain
            UNION
            SELECT e.member_account_set_id
            FROM target_reach r
            JOIN cala_account_set_member_account_sets e
                ON e.account_set_id = r.set_id
          )
          SELECT
            EXISTS (
                SELECT 1 FROM target_ancestors WHERE set_id = $2
            ) AS "cycle!",
            EXISTS (
                SELECT 1 FROM subtree_reach r
                JOIN target_chain t ON r.set_id = t.set_id
            ) AS "set_conflict!",
            EXISTS (
                SELECT 1
                FROM cala_account_set_member_accounts ma
                JOIN member_subtree ms ON ma.account_set_id = ms.set_id
                JOIN cala_account_set_member_accounts ta
                    ON ta.member_account_id = ma.member_account_id
                JOIN target_reach tr ON ta.account_set_id = tr.set_id
            ) AS "account_conflict!",
            COALESCE((SELECT MAX(depth) FROM target_ancestors), 0)
            + 1
            + COALESCE((SELECT MAX(depth) FROM member_subtree), 0) AS "depth!"
          "#,
            account_set_id as AccountSetId,
            member_account_set_id as AccountSetId,
            MAX_MEMBERSHIP_DEPTH,
        )
        .fetch_one(db.as_executor())
        .await?;
        if checks.cycle {
            return Err(AccountSetError::MembershipCycleDetected {
                account_set_id,
                member_account_set_id,
            });
        }
        if checks.set_conflict || checks.account_conflict {
            return Err(AccountSetError::MemberAlreadyAdded);
        }
        if checks.depth > MAX_MEMBERSHIP_DEPTH {
            return Err(AccountSetError::MembershipDepthExceeded {
                account_set_id,
                member_account_set_id,
                depth: checks.depth,
                max: MAX_MEMBERSHIP_DEPTH,
            });
        }

        // Insert the single direct set->set edge. Ancestor membership is
        // resolved by the read-time walk; there is no closure to propagate.
        sqlx::query!(
            r#"
          INSERT INTO cala_account_set_member_account_sets (account_set_id, member_account_set_id)
          VALUES ($1, $2)
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
        // Delete the single direct set->set edge. There are no
        // materialized ancestor/member rows to scrub.
        sqlx::query!(
            r#"
          DELETE FROM cala_account_set_member_account_sets
          WHERE account_set_id = $1 AND member_account_set_id = $2
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
              WHERE asm.member_account_id = $1
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
        level = "debug",
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
        // Adjacency-only membership: resolve each account's ancestor sets
        // by an upward recursive walk over the (tiny) set->set edge table,
        // seeded from the account's direct set memberships. UNION (not
        // UNION ALL) dedups and keeps the walk terminating even if a stray
        // edge slipped past the write-side cycle check.
        let rows = op.into_executor().fetch_all(sqlx::query!(
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
          SELECT a.account_id AS "account_id!: AccountId", a.account_set_id AS "set_id!: AccountSetId"
          FROM ancestors a
          JOIN cala_account_sets s
            ON s.id = a.account_set_id AND s.journal_id = $1
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
