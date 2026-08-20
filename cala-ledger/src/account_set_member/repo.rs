use sqlx::PgPool;

use crate::{
    outbox::{OutboxEventPayload, OutboxPublisher},
    primitives::{AccountId, AccountSetId},
};

use super::error::AccountSetMemberError;

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

use cala_types::account_set::{AccountSetMember, AccountSetMemberByExternalId, AccountSetMemberId};
use members_cursor::*;

/// `classid` namespace for the per-member advisory locks (2-arg form),
/// keyed on `hashtext(<member account id>)`. Must stay disjoint from
/// `EC_SET_LOCK_CLASS` (= 1, `balance` module) and `GRAPH_LOCK_CLASS`
/// (= 3, `account_set::repo`).
pub(crate) const MEMBER_LOCK_CLASS: i32 = 2;

/// The account-member edge (`cala_account_set_member_accounts`): sole write
/// authority (insert, delete, the class-2 lock) and the member-listing
/// reads. NOT an `EsRepo` — the edge is a plain relation plus an outbox
/// event, not an event-sourced entity.
///
/// Every mutation method is `_in_op`-only, deliberately: a bare
/// `add(set, account)` opening its own operation would be an unfenced
/// write primitive that bypasses the caller's lock protocol and
/// path-uniqueness validation. The two callers — the classic attach
/// protocol (`account_set` module) and the create-inside-set fast path
/// (`account` module, `NewAccount::initial_account_set`) — take their own
/// locks before calling in; see the doc comments on each method for its
/// precondition.
#[derive(Debug, Clone)]
pub(crate) struct AccountSetMemberRepo {
    pool: PgPool,
    publisher: OutboxPublisher,
}

impl AccountSetMemberRepo {
    pub fn new(pool: &PgPool, publisher: &OutboxPublisher) -> Self {
        Self {
            pool: pool.clone(),
            publisher: publisher.clone(),
        }
    }

    /// Take the EXCLUSIVE per-member ([`MEMBER_LOCK_CLASS`]) advisory locks
    /// for `account_ids`, one statement, for the CLASSIC attach/detach
    /// protocol (the caller has already taken the coarse SHARED lock —
    /// see `AccountSetRepo::lock_graph_shared_in_op`).
    ///
    /// Canonical acquisition order is established ON THE PG SIDE, not by
    /// the caller: a naive `ORDER BY` at the same query level as the
    /// volatile `pg_advisory_xact_lock` call is unsafe — the planner is
    /// free to evaluate the lock projection before any Sort node. The
    /// `ordered` CTE is marked `AS MATERIALIZED` as an explicit optimizer
    /// fence: PG materializes it into a tuplestore exactly as written
    /// (sorted, deduped), and the lock-bearing level above scans that
    /// tuplestore in insertion order, taking one lock per row in
    /// ascending `account_id` order. `AS MATERIALIZED` is load-bearing —
    /// without it a single-reference, non-volatile CTE is eligible for
    /// inlining (PG >= 12), which reopens the same-level hazard.
    pub(crate) async fn lock_members_in_op(
        &self,
        db: &mut impl es_entity::AtomicOperation,
        account_ids: &[AccountId],
    ) -> Result<(), sqlx::Error> {
        sqlx::query!(
            r#"
            WITH ordered AS MATERIALIZED (
                SELECT DISTINCT account_id
                FROM UNNEST($2::uuid[]) AS v(account_id)
                ORDER BY account_id
            )
            SELECT pg_advisory_xact_lock($1, hashtext(account_id::text))
            FROM ordered
            "#,
            MEMBER_LOCK_CLASS,
            account_ids as &[AccountId],
        )
        .execute(db.as_executor())
        .await?;
        Ok(())
    }

    /// Insert the direct account-member edges for every `(account_set_id,
    /// account_id)` pair — one statement — and publish one
    /// [`OutboxEventPayload::AccountSetMemberCreated`] per pair.
    ///
    /// Precondition (CLASSIC path only): the caller has taken the coarse
    /// SHARED lock, [`Self::lock_members_in_op`], and the path-uniqueness
    /// check for every member in this same op. The create-inside-set fast
    /// path does NOT call this — see
    /// [`Self::attach_new_accounts_in_op`].
    pub(crate) async fn add_in_op(
        &self,
        db: &mut impl es_entity::AtomicOperation,
        pairs: &[(AccountSetId, AccountId)],
    ) -> Result<(), sqlx::Error> {
        if pairs.is_empty() {
            return Ok(());
        }
        let account_set_ids: Vec<AccountSetId> = pairs.iter().map(|(set_id, _)| *set_id).collect();
        let account_ids: Vec<AccountId> = pairs.iter().map(|(_, account_id)| *account_id).collect();

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

        self.publish_created(db, pairs).await
    }

    /// The create-inside-set fast path (`Accounts::create_*` via
    /// `NewAccount::initial_account_set`): lock and insert in ONE
    /// statement, plus the outbox publish.
    ///
    /// Precondition: `pairs` names accounts created *in this same op* —
    /// no lock protocol beyond the class-2 lock this statement itself
    /// takes, and no path-uniqueness check. The invariant argument for
    /// why that is sound (k=1 memberships only) lives on
    /// `NewAccount::initial_account_set`'s field docs.
    ///
    /// The lock ordering fence is the same shape as
    /// [`Self::lock_members_in_op`] (see its docs) — sorted one level
    /// below the lock call inside `AS MATERIALIZED`. No `DISTINCT`: each
    /// account appears at most once by construction (one target set per
    /// `NewAccount`).
    ///
    /// The `account_set_id` FK IS the set-existence check — race-free and
    /// authoritative. On an FK violation naming that constraint, this
    /// re-probes ([`Self::missing_account_sets`]) to report every missing
    /// id; the member-account FK cannot fire here (the account rows were
    /// inserted earlier in the same op).
    pub(crate) async fn attach_new_accounts_in_op(
        &self,
        db: &mut impl es_entity::AtomicOperation,
        pairs: &[(AccountSetId, AccountId)],
    ) -> Result<(), AccountSetMemberError> {
        if pairs.is_empty() {
            return Ok(());
        }
        let account_set_ids: Vec<AccountSetId> = pairs.iter().map(|(set_id, _)| *set_id).collect();
        let account_ids: Vec<AccountId> = pairs.iter().map(|(_, account_id)| *account_id).collect();

        let result = sqlx::query!(
            r#"
            WITH ordered AS MATERIALIZED (
                SELECT v.account_set_id, v.account_id
                FROM UNNEST($1::uuid[], $2::uuid[]) AS v(account_set_id, account_id)
                ORDER BY v.account_id
            ), locked AS MATERIALIZED (
                SELECT account_set_id, account_id,
                       pg_advisory_xact_lock($3, hashtext(account_id::text))
                FROM ordered
            )
            INSERT INTO cala_account_set_member_accounts (account_set_id, member_account_id)
            SELECT account_set_id, account_id
            FROM locked
            "#,
            &account_set_ids as &[AccountSetId],
            &account_ids as &[AccountId],
            MEMBER_LOCK_CLASS,
        )
        .execute(db.as_executor())
        .await;

        match result {
            Ok(_) => {}
            Err(sqlx::Error::Database(e)) if is_account_set_fk_violation(e.as_ref()) => {
                let missing = self.missing_account_sets(&account_set_ids).await?;
                return Err(AccountSetMemberError::AccountSetsNotFound(missing));
            }
            Err(e) => return Err(e.into()),
        }

        self.publish_created(db, pairs).await?;
        Ok(())
    }

    /// Deliberately takes NO operation/executor: this only ever runs
    /// after its caller's insert has already failed, so it reads
    /// committed state on `self.pool` — a connection independent of the
    /// caller's now-aborted transaction, which cannot accept further
    /// statements until it rolls back.
    async fn missing_account_sets(
        &self,
        set_ids: &[AccountSetId],
    ) -> Result<Vec<AccountSetId>, sqlx::Error> {
        let found = sqlx::query!(
            r#"SELECT id AS "id: AccountSetId" FROM cala_account_sets WHERE id = ANY($1)"#,
            set_ids as &[AccountSetId],
        )
        .fetch_all(&self.pool)
        .await?;
        let found: std::collections::HashSet<AccountSetId> =
            found.into_iter().map(|row| row.id).collect();
        let mut missing: Vec<AccountSetId> = set_ids
            .iter()
            .filter(|id| !found.contains(id))
            .copied()
            .collect();
        missing.sort_unstable();
        missing.dedup();
        Ok(missing)
    }

    async fn publish_created(
        &self,
        db: &mut impl es_entity::AtomicOperation,
        pairs: &[(AccountSetId, AccountId)],
    ) -> Result<(), sqlx::Error> {
        self.publisher
            .publish_all(
                db,
                pairs.iter().map(|(account_set_id, account_id)| {
                    OutboxEventPayload::AccountSetMemberCreated {
                        account_set_id: *account_set_id,
                        member_id: AccountSetMemberId::Account(*account_id),
                    }
                }),
            )
            .await
    }

    /// Delete the single direct edge (plus the outbox event).
    ///
    /// Precondition: the caller has taken the coarse SHARED lock and
    /// [`Self::lock_members_in_op`] for `account_id` in this same op — the
    /// lock keeps same-member add/remove interleavings serialized.
    pub(crate) async fn remove_in_op(
        &self,
        db: &mut impl es_entity::AtomicOperation,
        account_set_id: AccountSetId,
        account_id: AccountId,
    ) -> Result<(), sqlx::Error> {
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
                std::iter::once(OutboxEventPayload::AccountSetMemberRemoved {
                    account_set_id,
                    member_id: AccountSetMemberId::Account(account_id),
                }),
            )
            .await?;

        Ok(())
    }

    pub(crate) async fn list_by_created_at(
        &self,
        id: AccountSetId,
        args: es_entity::PaginatedQueryArgs<AccountSetMemberByCreatedAtCursor>,
    ) -> Result<
        es_entity::PaginatedQueryRet<AccountSetMember, AccountSetMemberByCreatedAtCursor>,
        sqlx::Error,
    > {
        self.list_by_created_at_in_op(&self.pool, id, args).await
    }

    pub(crate) async fn list_by_created_at_in_op(
        &self,
        op: impl es_entity::IntoOneTimeExecutor<'_>,
        account_set_id: AccountSetId,
        args: es_entity::PaginatedQueryArgs<AccountSetMemberByCreatedAtCursor>,
    ) -> Result<
        es_entity::PaginatedQueryRet<AccountSetMember, AccountSetMemberByCreatedAtCursor>,
        sqlx::Error,
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

    pub(crate) async fn list_by_external_id(
        &self,
        id: AccountSetId,
        args: es_entity::PaginatedQueryArgs<AccountSetMemberByExternalIdCursor>,
    ) -> Result<
        es_entity::PaginatedQueryRet<
            AccountSetMemberByExternalId,
            AccountSetMemberByExternalIdCursor,
        >,
        sqlx::Error,
    > {
        self.list_by_external_id_in_op(&self.pool, id, args).await
    }

    pub(crate) async fn list_by_external_id_in_op(
        &self,
        op: impl es_entity::IntoOneTimeExecutor<'_>,
        account_set_id: AccountSetId,
        args: es_entity::PaginatedQueryArgs<AccountSetMemberByExternalIdCursor>,
    ) -> Result<
        es_entity::PaginatedQueryRet<
            AccountSetMemberByExternalId,
            AccountSetMemberByExternalIdCursor,
        >,
        sqlx::Error,
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
}

/// `true` iff `err` is the FK violation on
/// `cala_account_set_member_accounts.account_set_id` — i.e. an
/// `attach_new_accounts_in_op` insert named a nonexistent account set.
/// Matched by substring on the (unnamed-in-migration, PG-autogenerated)
/// constraint name, matching this crate's existing convention for
/// matching constraint names (see `AccountSetError`'s
/// `From<sqlx::Error>`) rather than the full name, so a future rename
/// that keeps "account_set_id_fkey" as a suffix does not silently stop
/// matching.
fn is_account_set_fk_violation(err: &dyn sqlx::error::DatabaseError) -> bool {
    err.constraint()
        .is_some_and(|c| c.contains("member_accounts_account_set_id_fkey"))
}
