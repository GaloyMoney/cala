use chrono::{DateTime, Utc};
use es_entity::*;
use sqlx::PgPool;

use std::collections::HashMap;

use crate::primitives::{AccountId, DataSourceId, JournalId};

use super::{entity::*, error::*};

const ADDVISORY_LOCK_ID: i64 = 123456;

pub mod members_cursor {
    use cala_types::account_set::{
        AccountSetMember, AccountSetMemberByExternalId, AccountSetMemberId,
    };
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Serialize, Deserialize)]
    pub struct AccountSetMembersByCreatedAtCursor {
        pub id: AccountSetMemberId,
        pub member_created_at: chrono::DateTime<chrono::Utc>,
    }

    impl From<&AccountSetMember> for AccountSetMembersByCreatedAtCursor {
        fn from(member: &AccountSetMember) -> Self {
            Self {
                id: member.id,
                member_created_at: member.created_at,
            }
        }
    }

    #[cfg(feature = "graphql")]
    impl async_graphql::connection::CursorType for AccountSetMembersByCreatedAtCursor {
        type Error = String;

        fn encode_cursor(&self) -> String {
            use base64::{engine::general_purpose, Engine as _};
            let json = serde_json::to_string(&self).expect("could not serialize token");
            general_purpose::STANDARD_NO_PAD.encode(json.as_bytes())
        }

        fn decode_cursor(s: &str) -> Result<Self, Self::Error> {
            use base64::{engine::general_purpose, Engine as _};
            let bytes = general_purpose::STANDARD_NO_PAD
                .decode(s.as_bytes())
                .map_err(|e| e.to_string())?;
            let json = String::from_utf8(bytes).map_err(|e| e.to_string())?;
            serde_json::from_str(&json).map_err(|e| e.to_string())
        }
    }

    #[derive(Debug, Serialize, Deserialize)]
    pub struct AccountSetMembersByExternalIdCursor {
        pub id: AccountSetMemberId,
        pub external_id: Option<String>,
    }

    impl From<&AccountSetMemberByExternalId> for AccountSetMembersByExternalIdCursor {
        fn from(member: &AccountSetMemberByExternalId) -> Self {
            Self {
                id: member.id,
                external_id: member.external_id.clone(),
            }
        }
    }

    #[cfg(feature = "graphql")]
    impl async_graphql::connection::CursorType for AccountSetMembersByExternalIdCursor {
        type Error = String;

        fn encode_cursor(&self) -> String {
            use base64::{engine::general_purpose, Engine as _};
            let json = serde_json::to_string(&self).expect("could not serialize token");
            general_purpose::STANDARD_NO_PAD.encode(json.as_bytes())
        }

        fn decode_cursor(s: &str) -> Result<Self, Self::Error> {
            use base64::{engine::general_purpose, Engine as _};
            let bytes = general_purpose::STANDARD_NO_PAD
                .decode(s.as_bytes())
                .map_err(|e| e.to_string())?;
            let json = String::from_utf8(bytes).map_err(|e| e.to_string())?;
            serde_json::from_str(&json).map_err(|e| e.to_string())
        }
    }
}

use account_set_cursor::*;
use members_cursor::*;

#[derive(EsRepo, Debug, Clone)]
#[es_repo(
    entity = "AccountSet",
    err = "AccountSetError",
    columns(
        name(ty = "String", update(accessor = "values().name"), list_by, list_for),
        journal_id(ty = "JournalId", update(persist = false)),
        external_id(
            ty = "Option<String>",
            update(accessor = "values().external_id"),
            list_by
        ),
        data_source_id(
            ty = "DataSourceId",
            create(accessor = "data_source().into()"),
            update(persist = false)
        ),
    ),
    tbl_prefix = "cala"
)]
pub(super) struct AccountSetRepo {
    pool: PgPool,
}

impl AccountSetRepo {
    pub fn new(pool: &PgPool) -> Self {
        Self { pool: pool.clone() }
    }

    pub async fn list_children_by_created_at(
        &self,
        id: AccountSetId,
        args: es_entity::PaginatedQueryArgs<AccountSetMembersByCreatedAtCursor>,
    ) -> Result<
        es_entity::PaginatedQueryRet<AccountSetMember, AccountSetMembersByCreatedAtCursor>,
        AccountSetError,
    > {
        self.list_children_by_created_at_in_op(&self.pool, id, args)
            .await
    }

    pub async fn list_children_by_created_at_in_op(
        &self,
        op: impl es_entity::IntoExecutor<'_>,
        account_set_id: AccountSetId,
        args: es_entity::PaginatedQueryArgs<AccountSetMembersByCreatedAtCursor>,
    ) -> Result<
        es_entity::PaginatedQueryRet<AccountSetMember, AccountSetMembersByCreatedAtCursor>,
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

        let rows = sqlx::query!(
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
        )
        .fetch_all(op.into_executor())
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
            end_cursor = Some(AccountSetMembersByCreatedAtCursor {
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
        args: es_entity::PaginatedQueryArgs<AccountSetMembersByExternalIdCursor>,
    ) -> Result<
        es_entity::PaginatedQueryRet<
            AccountSetMemberByExternalId,
            AccountSetMembersByExternalIdCursor,
        >,
        AccountSetError,
    > {
        self.list_children_by_external_id_in_op(&self.pool, id, args)
            .await
    }

    pub async fn list_children_by_external_id_in_op(
        &self,
        op: impl es_entity::IntoExecutor<'_>,
        account_set_id: AccountSetId,
        args: es_entity::PaginatedQueryArgs<AccountSetMembersByExternalIdCursor>,
    ) -> Result<
        es_entity::PaginatedQueryRet<
            AccountSetMemberByExternalId,
            AccountSetMembersByExternalIdCursor,
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

        let rows = sqlx::query!(
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
        )
        .fetch_all(op.into_executor())
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
            end_cursor = Some(AccountSetMembersByExternalIdCursor {
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

    pub async fn add_member_account_and_return_parents(
        &self,
        db: &mut impl es_entity::AtomicOperation,
        account_set_id: AccountSetId,
        account_id: AccountId,
    ) -> Result<(DateTime<Utc>, Vec<AccountSetId>), AccountSetError> {
        sqlx::query!("SELECT pg_advisory_xact_lock($1)", ADDVISORY_LOCK_ID)
            .execute(db.as_executor())
            .await?;
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
        Ok((time.expect("time not set"), ret))
    }

    pub async fn remove_member_account_and_return_parents(
        &self,
        db: &mut impl es_entity::AtomicOperation,
        account_set_id: AccountSetId,
        account_id: AccountId,
    ) -> Result<(DateTime<Utc>, Vec<AccountSetId>), AccountSetError> {
        sqlx::query!("SELECT pg_advisory_xact_lock($1)", ADDVISORY_LOCK_ID)
            .execute(db.as_executor())
            .await?;
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
        Ok((time.expect("time not set"), ret))
    }

    pub async fn add_member_set_and_return_parents(
        &self,
        db: &mut impl es_entity::AtomicOperation,
        account_set_id: AccountSetId,
        member_account_set_id: AccountSetId,
    ) -> Result<(DateTime<Utc>, Vec<AccountSetId>), AccountSetError> {
        sqlx::query!("SELECT pg_advisory_xact_lock($1)", ADDVISORY_LOCK_ID)
            .execute(db.as_executor())
            .await?;
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
        Ok((time.expect("time not set"), ret))
    }

    pub async fn remove_member_set_and_return_parents(
        &self,
        db: &mut impl es_entity::AtomicOperation,
        account_set_id: AccountSetId,
        member_account_set_id: AccountSetId,
    ) -> Result<(DateTime<Utc>, Vec<AccountSetId>), AccountSetError> {
        sqlx::query!("SELECT pg_advisory_xact_lock($1)", ADDVISORY_LOCK_ID)
            .execute(db.as_executor())
            .await?;
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
        Ok((time.expect("time not set"), ret))
    }

    pub async fn find_where_account_is_member(
        &self,
        account_id: AccountId,
        query: es_entity::PaginatedQueryArgs<AccountSetsByNameCursor>,
    ) -> Result<es_entity::PaginatedQueryRet<AccountSet, AccountSetsByNameCursor>, AccountSetError>
    {
        self.find_where_account_is_member_in_op(&self.pool, account_id, query)
            .await
    }

    pub async fn find_where_account_is_member_in_op(
        &self,
        op: impl es_entity::IntoExecutor<'_>,
        account_id: AccountId,
        query: es_entity::PaginatedQueryArgs<AccountSetsByNameCursor>,
    ) -> Result<es_entity::PaginatedQueryRet<AccountSet, AccountSetsByNameCursor>, AccountSetError>
    {
        let rows = sqlx::query_as!(
            account_set_repo_types::Repo__DbEvent,
            r#"
            WITH member_account_sets AS (
              SELECT a.id, a.name, a.created_at
              FROM cala_account_set_member_accounts asm
              JOIN cala_account_sets a ON asm.account_set_id = a.id
              WHERE asm.member_account_id = $1 AND transitive IS FALSE
              AND ((a.name, a.id) > ($3, $2) OR ($3 IS NULL AND $2 IS NULL))
              ORDER BY a.name, a.id
              LIMIT $4
            )
            SELECT mas.id AS "entity_id!: AccountSetId", e.sequence, e.event, e.recorded_at
              FROM member_account_sets mas
              JOIN cala_account_set_events e ON mas.id = e.id
              ORDER BY mas.name, mas.id, e.sequence
            "#,
            account_id as AccountId,
            query.after.as_ref().map(|c| c.id) as Option<AccountSetId>,
            query.after.map(|c| c.name),
            query.first as i64 + 1
        )
        .fetch_all(op.into_executor())
        .await?;

        let (entities, has_next_page) = EntityEvents::load_n::<AccountSet>(rows, query.first)?;
        let mut end_cursor = None;
        if let Some(last) = entities.last() {
            end_cursor = Some(AccountSetsByNameCursor {
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
        query: es_entity::PaginatedQueryArgs<AccountSetsByNameCursor>,
    ) -> Result<es_entity::PaginatedQueryRet<AccountSet, AccountSetsByNameCursor>, AccountSetError>
    {
        self.find_where_account_set_is_member_in_op(&self.pool, account_set_id, query)
            .await
    }

    pub async fn find_where_account_set_is_member_in_op(
        &self,
        op: impl es_entity::IntoExecutor<'_>,
        account_set_id: AccountSetId,
        query: es_entity::PaginatedQueryArgs<AccountSetsByNameCursor>,
    ) -> Result<es_entity::PaginatedQueryRet<AccountSet, AccountSetsByNameCursor>, AccountSetError>
    {
        let rows = sqlx::query_as!(
            account_set_repo_types::Repo__DbEvent,
            r#"
            WITH member_account_sets AS (
              SELECT a.id, a.name, a.created_at
              FROM cala_account_set_member_account_sets asm
              JOIN cala_account_sets a ON asm.account_set_id = a.id
              WHERE asm.member_account_set_id = $1
              AND ((a.name, a.id) > ($3, $2) OR ($3 IS NULL AND $2 IS NULL))
              ORDER BY a.name, a.id
              LIMIT $4
            )
            SELECT mas.id AS "entity_id!: AccountSetId", e.sequence, e.event, e.recorded_at
              FROM member_account_sets mas
              JOIN cala_account_set_events e ON mas.id = e.id
              ORDER BY mas.name, mas.id, e.sequence
            "#,
            account_set_id as AccountSetId,
            query.after.as_ref().map(|c| c.id) as Option<AccountSetId>,
            query.after.map(|c| c.name),
            query.first as i64 + 1
        )
        .fetch_all(op.into_executor())
        .await?;

        let (entities, has_next_page) = EntityEvents::load_n::<AccountSet>(rows, query.first)?;
        let mut end_cursor = None;
        if let Some(last) = entities.last() {
            end_cursor = Some(AccountSetsByNameCursor {
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

    #[cfg(feature = "import")]
    pub async fn import_in_op(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        origin: DataSourceId,
        account_set: &mut AccountSet,
    ) -> Result<(), AccountSetError> {
        let recorded_at = op.now();
        sqlx::query!(
            r#"INSERT INTO cala_account_sets (data_source_id, id, journal_id, name, external_id, created_at)
            VALUES ($1, $2, $3, $4, $5, $6)"#,
            origin as DataSourceId,
            account_set.values().id as AccountSetId,
            account_set.values().journal_id as JournalId,
            account_set.values().name,
            account_set.values().external_id,
            recorded_at
        )
        .execute(op.as_executor())
        .await?;
        self.persist_events(op, account_set.events_mut()).await?;
        Ok(())
    }

    pub async fn fetch_mappings_in_op(
        &self,
        op: impl es_entity::IntoExecutor<'_>,
        journal_id: JournalId,
        account_ids: &[AccountId],
    ) -> Result<HashMap<AccountId, Vec<AccountSetId>>, AccountSetError> {
        let rows = sqlx::query!(
            r#"
          SELECT m.account_set_id AS "set_id!: AccountSetId", m.member_account_id AS "account_id!: AccountId"
          FROM cala_account_set_member_accounts m
          JOIN cala_account_sets s
          ON m.account_set_id = s.id AND s.journal_id = $1
          WHERE m.member_account_id = ANY($2)
          "#,
            journal_id as JournalId,
            account_ids as &[AccountId]
        )
        .fetch_all(op.into_executor())
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

    #[cfg(feature = "import")]
    pub async fn import_member_account_in_op(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        account_set_id: AccountSetId,
        account_id: AccountId,
    ) -> Result<(), AccountSetError> {
        let recorded_at = op.now();
        sqlx::query!(
            r#"INSERT INTO cala_account_set_member_accounts (account_set_id, member_account_id, created_at)
            VALUES ($1, $2, $3)"#,
            account_set_id as AccountSetId,
            account_id as AccountId,
            recorded_at
        )
        .execute(op.as_executor())
        .await?;
        Ok(())
    }

    #[cfg(feature = "import")]
    pub async fn import_remove_member_account(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        account_set_id: AccountSetId,
        account_id: AccountId,
    ) -> Result<(), AccountSetError> {
        sqlx::query!(
            r#"DELETE FROM cala_account_set_member_accounts
            WHERE account_set_id = $1 AND member_account_id = $2"#,
            account_set_id as AccountSetId,
            account_id as AccountId,
        )
        .execute(op.as_executor())
        .await?;
        Ok(())
    }

    #[cfg(feature = "import")]
    pub async fn import_member_set_in_op(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        account_set_id: AccountSetId,
        member_account_set_id: AccountSetId,
    ) -> Result<(), AccountSetError> {
        let recorded_at = op.now();
        sqlx::query!(
            r#"INSERT INTO cala_account_set_member_account_sets (account_set_id, member_account_set_id, created_at)
            VALUES ($1, $2, $3)"#,
            account_set_id as AccountSetId,
            member_account_set_id as AccountSetId,
            recorded_at
        )
        .execute(op.as_executor())
        .await?;
        Ok(())
    }

    #[cfg(feature = "import")]
    pub async fn import_remove_member_set(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        account_set_id: AccountSetId,
        member_account_set_id: AccountSetId,
    ) -> Result<(), AccountSetError> {
        sqlx::query!(
            r#"DELETE FROM cala_account_set_member_account_sets
            WHERE account_set_id = $1 AND member_account_set_id = $2"#,
            account_set_id as AccountSetId,
            member_account_set_id as AccountSetId,
        )
        .execute(op.as_executor())
        .await?;
        Ok(())
    }
}
