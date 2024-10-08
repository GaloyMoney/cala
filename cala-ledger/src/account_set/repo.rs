use chrono::{DateTime, Utc};
use sqlx::{Executor, PgPool, Postgres, Transaction};

use std::collections::HashMap;

#[cfg(feature = "import")]
use crate::primitives::DataSourceId;
use crate::{
    entity::*,
    primitives::{AccountId, JournalId},
    query,
};

use super::{cursor::*, entity::*, error::*, AccountSetByNameCursor};

const ADDVISORY_LOCK_ID: i64 = 123456;

#[derive(Debug, Clone)]
pub(super) struct AccountSetRepo {
    pool: PgPool,
}

impl AccountSetRepo {
    pub fn new(pool: &PgPool) -> Self {
        Self { pool: pool.clone() }
    }

    pub async fn create_in_tx(
        &self,
        db: &mut Transaction<'_, Postgres>,
        new_account_set: NewAccountSet,
    ) -> Result<AccountSet, AccountSetError> {
        sqlx::query!(
            r#"INSERT INTO cala_account_sets (id, journal_id, name)
            VALUES ($1, $2, $3)"#,
            new_account_set.id as AccountSetId,
            new_account_set.journal_id as JournalId,
            new_account_set.name,
        )
        .execute(&mut **db)
        .await?;
        let mut events = new_account_set.initial_events();
        events.persist(db).await?;
        let account_set = AccountSet::try_from(events)?;
        Ok(account_set)
    }

    pub async fn list_children(
        &self,
        id: AccountSetId,
        args: query::PaginatedQueryArgs<AccountSetMemberCursor>,
    ) -> Result<query::PaginatedQueryRet<AccountSetMember, AccountSetMemberCursor>, AccountSetError>
    {
        self.list_children_in_executor(&self.pool, id, args).await
    }

    pub async fn list_children_in_tx(
        &self,
        db: &mut Transaction<'_, Postgres>,
        id: AccountSetId,
        args: query::PaginatedQueryArgs<AccountSetMemberCursor>,
    ) -> Result<query::PaginatedQueryRet<AccountSetMember, AccountSetMemberCursor>, AccountSetError>
    {
        self.list_children_in_executor(&mut **db, id, args).await
    }

    async fn list_children_in_executor(
        &self,
        executor: impl Executor<'_, Database = Postgres>,
        id: AccountSetId,
        args: query::PaginatedQueryArgs<AccountSetMemberCursor>,
    ) -> Result<query::PaginatedQueryRet<AccountSetMember, AccountSetMemberCursor>, AccountSetError>
    {
        let after = args.after.map(|c| c.member_created_at) as Option<DateTime<Utc>>;
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
                AND account_set_id = $1
                AND (created_at < $2 OR $2 IS NULL)
              ORDER BY created_at DESC
              LIMIT $3
            ), member_sets AS (
              SELECT
                member_account_set_id AS member_id,
                NULL::uuid AS member_account_id,
                member_account_set_id,
                created_at
              FROM cala_account_set_member_account_sets
              WHERE
                account_set_id = $1
                AND (created_at < $2 OR $2 IS NULL)
              ORDER BY created_at DESC
              LIMIT $3
            ), all_members AS (
              SELECT * FROM member_accounts
              UNION ALL
              SELECT * FROM member_sets
            )
            SELECT * FROM all_members
            ORDER BY created_at DESC
            LIMIT $3
          "#,
            id as AccountSetId,
            after,
            args.first as i64 + 1,
        )
        .fetch_all(executor)
        .await?;
        let has_next_page = rows.len() > args.first;
        let mut end_cursor = None;
        if let Some(last) = rows.last() {
            end_cursor = Some(AccountSetMemberCursor {
                member_created_at: last.created_at.expect("created_at not set"),
            });
        }

        let account_set_members = rows
            .into_iter()
            .take(args.first)
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

        Ok(query::PaginatedQueryRet {
            entities: account_set_members,
            has_next_page,
            end_cursor,
        })
    }

    pub async fn add_member_account_and_return_parents(
        &self,
        db: &mut Transaction<'_, Postgres>,
        account_set_id: AccountSetId,
        account_id: AccountId,
    ) -> Result<(DateTime<Utc>, Vec<AccountSetId>), AccountSetError> {
        sqlx::query!("SELECT pg_advisory_xact_lock($1)", ADDVISORY_LOCK_ID)
            .execute(&mut **db)
            .await?;
        let rows = sqlx::query!(r#"
          WITH RECURSIVE parents AS (
            SELECT m.member_account_set_id, m.account_set_id, s.data_source_id
            FROM cala_account_set_member_account_sets m
            JOIN cala_account_sets s
            ON s.id = m.account_set_id AND s.data_source_id = m.data_source_id
            WHERE s.data_source_id = '00000000-0000-0000-0000-000000000000'
            AND m.member_account_set_id = $1

            UNION ALL
            SELECT p.member_account_set_id, m.account_set_id, m.data_source_id
            FROM parents p
            JOIN cala_account_set_member_account_sets m
                ON p.account_set_id = m.member_account_set_id
                AND p.data_source_id = m.data_source_id
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
        .fetch_all(&mut **db)
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
        db: &mut Transaction<'_, Postgres>,
        account_set_id: AccountSetId,
        account_id: AccountId,
    ) -> Result<(DateTime<Utc>, Vec<AccountSetId>), AccountSetError> {
        sqlx::query!("SELECT pg_advisory_xact_lock($1)", ADDVISORY_LOCK_ID)
            .execute(&mut **db)
            .await?;
        let rows = sqlx::query!(
            r#"
          WITH RECURSIVE parents AS (
            SELECT m.member_account_set_id, m.account_set_id, s.data_source_id
            FROM cala_account_set_member_account_sets m
            JOIN cala_account_sets s
            ON s.id = m.account_set_id AND s.data_source_id = m.data_source_id
            WHERE s.data_source_id = '00000000-0000-0000-0000-000000000000'
            AND m.member_account_set_id = $1

            UNION ALL
            SELECT p.member_account_set_id, m.account_set_id, m.data_source_id
            FROM parents p
            JOIN cala_account_set_member_account_sets m
                ON p.account_set_id = m.member_account_set_id
                AND p.data_source_id = m.data_source_id
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
        .fetch_all(&mut **db)
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
        db: &mut Transaction<'_, Postgres>,
        account_set_id: AccountSetId,
        member_account_set_id: AccountSetId,
    ) -> Result<(DateTime<Utc>, Vec<AccountSetId>), AccountSetError> {
        sqlx::query!("SELECT pg_advisory_xact_lock($1)", ADDVISORY_LOCK_ID)
            .execute(&mut **db)
            .await?;
        let rows = sqlx::query!(r#"
          WITH RECURSIVE parents AS (
            SELECT m.member_account_set_id, m.account_set_id, s.data_source_id
            FROM cala_account_set_member_account_sets m
            JOIN cala_account_sets s
            ON s.id = m.account_set_id AND s.data_source_id = m.data_source_id
            WHERE s.data_source_id = '00000000-0000-0000-0000-000000000000'
            AND m.member_account_set_id = $1

            UNION ALL
            SELECT p.member_account_set_id, m.account_set_id, m.data_source_id
            FROM parents p
            JOIN cala_account_set_member_account_sets m
                ON p.account_set_id = m.member_account_set_id
                AND p.data_source_id = m.data_source_id
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
            AND m.data_source_id = '00000000-0000-0000-0000-000000000000'
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
        .fetch_all(&mut **db)
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
        db: &mut Transaction<'_, Postgres>,
        account_set_id: AccountSetId,
        member_account_set_id: AccountSetId,
    ) -> Result<(DateTime<Utc>, Vec<AccountSetId>), AccountSetError> {
        sqlx::query!("SELECT pg_advisory_xact_lock($1)", ADDVISORY_LOCK_ID)
            .execute(&mut **db)
            .await?;
        let rows = sqlx::query!(
            r#"
          WITH RECURSIVE parents AS (
            SELECT m.member_account_set_id, m.account_set_id, s.data_source_id
            FROM cala_account_set_member_account_sets m
            JOIN cala_account_sets s
            ON s.id = m.account_set_id AND s.data_source_id = m.data_source_id
            WHERE s.data_source_id = '00000000-0000-0000-0000-000000000000'
            AND m.member_account_set_id = $1

            UNION ALL
            SELECT p.member_account_set_id, m.account_set_id, m.data_source_id
            FROM parents p
            JOIN cala_account_set_member_account_sets m
                ON p.account_set_id = m.member_account_set_id
                AND p.data_source_id = m.data_source_id
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
        .fetch_all(&mut **db)
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

    pub async fn persist_in_tx(
        &self,
        db: &mut Transaction<'_, Postgres>,
        account_set: &mut AccountSet,
    ) -> Result<(), AccountSetError> {
        sqlx::query!(
            r#"UPDATE cala_account_sets
            SET name = $2
            WHERE id = $1 AND data_source_id = '00000000-0000-0000-0000-000000000000'"#,
            account_set.values().id as AccountSetId,
            account_set.values().name,
        )
        .execute(&mut **db)
        .await?;
        account_set.events.persist(db).await?;
        Ok(())
    }

    pub async fn find(&self, account_set_id: AccountSetId) -> Result<AccountSet, AccountSetError> {
        let mut tx = self.pool.begin().await?;
        self.find_in_tx(&mut tx, account_set_id).await
    }

    pub async fn find_in_tx(
        &self,
        db: &mut Transaction<'_, Postgres>,
        account_set_id: AccountSetId,
    ) -> Result<AccountSet, AccountSetError> {
        let rows = sqlx::query_as!(
            GenericEvent,
            r#"SELECT a.id, e.sequence, e.event,
                a.created_at AS entity_created_at, e.recorded_at AS event_recorded_at
            FROM cala_account_sets a
            JOIN cala_account_set_events e
            ON a.data_source_id = e.data_source_id
            AND a.id = e.id
            WHERE a.data_source_id = '00000000-0000-0000-0000-000000000000'
            AND a.id = $1
            ORDER BY e.sequence"#,
            account_set_id as AccountSetId
        )
        .fetch_all(&mut **db)
        .await?;
        match EntityEvents::load_first(rows) {
            Ok(account_set) => Ok(account_set),
            Err(EntityError::NoEntityEventsPresent) => {
                Err(AccountSetError::CouldNotFindById(account_set_id))
            }
            Err(e) => Err(e.into()),
        }
    }

    pub async fn find_where_account_is_member(
        &self,
        account_id: AccountId,
        query: query::PaginatedQueryArgs<AccountSetByNameCursor>,
    ) -> Result<query::PaginatedQueryRet<AccountSet, AccountSetByNameCursor>, AccountSetError> {
        self.find_where_account_is_member_in_executor(&self.pool, account_id, query)
            .await
    }

    pub async fn find_where_account_is_member_in_tx(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        account_id: AccountId,
        query: query::PaginatedQueryArgs<AccountSetByNameCursor>,
    ) -> Result<query::PaginatedQueryRet<AccountSet, AccountSetByNameCursor>, AccountSetError> {
        self.find_where_account_is_member_in_executor(&mut **tx, account_id, query)
            .await
    }

    async fn find_where_account_is_member_in_executor(
        &self,
        executor: impl Executor<'_, Database = Postgres>,
        account_id: AccountId,
        query: query::PaginatedQueryArgs<AccountSetByNameCursor>,
    ) -> Result<query::PaginatedQueryRet<AccountSet, AccountSetByNameCursor>, AccountSetError> {
        let rows = sqlx::query_as!(
            GenericEvent,
            r#"
            WITH member_account_sets AS (
              SELECT a.id, a.name, a.created_at
              FROM cala_account_set_member_accounts asm
              JOIN cala_account_sets a ON asm.data_source_id = a.data_source_id AND
              asm.account_set_id = a.id
              WHERE asm.data_source_id = '00000000-0000-0000-0000-000000000000' AND
              asm.member_account_id = $1 AND transitive IS FALSE
              AND ((a.name, a.id) > ($3, $2) OR ($3 IS NULL AND $2 IS NULL))
              ORDER BY a.name, a.id
              LIMIT $4
            )
            SELECT mas.id, e.sequence, e.event,
              mas.created_at AS entity_created_at, e.recorded_at AS event_recorded_at
              FROM member_account_sets mas
              JOIN cala_account_set_events e ON mas.id = e.id
              ORDER BY mas.name, mas.id, e.sequence
            "#,
            account_id as AccountId,
            query.after.as_ref().map(|c| c.id) as Option<AccountSetId>,
            query.after.map(|c| c.name),
            query.first as i64 + 1
        )
        .fetch_all(executor)
        .await?;

        let (entities, has_next_page) = EntityEvents::load_n::<AccountSet>(rows, query.first)?;
        let mut end_cursor = None;
        if let Some(last) = entities.last() {
            end_cursor = Some(AccountSetByNameCursor {
                id: last.values().id,
                name: last.values().name.clone(),
            });
        }
        Ok(query::PaginatedQueryRet {
            entities,
            has_next_page,
            end_cursor,
        })
    }

    pub async fn find_where_account_set_is_member(
        &self,
        account_set_id: AccountSetId,
        query: query::PaginatedQueryArgs<AccountSetByNameCursor>,
    ) -> Result<query::PaginatedQueryRet<AccountSet, AccountSetByNameCursor>, AccountSetError> {
        self.find_where_account_set_is_member_in_executor(&self.pool, account_set_id, query)
            .await
    }

    pub async fn find_where_account_set_is_member_in_tx(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        account_set_id: AccountSetId,
        query: query::PaginatedQueryArgs<AccountSetByNameCursor>,
    ) -> Result<query::PaginatedQueryRet<AccountSet, AccountSetByNameCursor>, AccountSetError> {
        self.find_where_account_set_is_member_in_executor(&mut **tx, account_set_id, query)
            .await
    }

    async fn find_where_account_set_is_member_in_executor(
        &self,
        executor: impl Executor<'_, Database = Postgres>,
        account_set_id: AccountSetId,
        query: query::PaginatedQueryArgs<AccountSetByNameCursor>,
    ) -> Result<query::PaginatedQueryRet<AccountSet, AccountSetByNameCursor>, AccountSetError> {
        let rows = sqlx::query_as!(
            GenericEvent,
            r#"
            WITH member_account_sets AS (
              SELECT a.id, a.name, a.created_at
              FROM cala_account_set_member_account_sets asm
              JOIN cala_account_sets a ON asm.data_source_id = a.data_source_id AND
              asm.account_set_id = a.id
              WHERE asm.data_source_id = '00000000-0000-0000-0000-000000000000' AND
              asm.member_account_set_id = $1
              AND ((a.name, a.id) > ($3, $2) OR ($3 IS NULL AND $2 IS NULL))
              ORDER BY a.name, a.id
              LIMIT $4
            )
            SELECT mas.id, e.sequence, e.event,
              mas.created_at AS entity_created_at, e.recorded_at AS event_recorded_at
              FROM member_account_sets mas
              JOIN cala_account_set_events e ON mas.id = e.id
              ORDER BY mas.name, mas.id, e.sequence
            "#,
            account_set_id as AccountSetId,
            query.after.as_ref().map(|c| c.id) as Option<AccountSetId>,
            query.after.map(|c| c.name),
            query.first as i64 + 1
        )
        .fetch_all(executor)
        .await?;

        let (entities, has_next_page) = EntityEvents::load_n::<AccountSet>(rows, query.first)?;
        let mut end_cursor = None;
        if let Some(last) = entities.last() {
            end_cursor = Some(AccountSetByNameCursor {
                id: last.values().id,
                name: last.values().name.clone(),
            });
        }
        Ok(query::PaginatedQueryRet {
            entities,
            has_next_page,
            end_cursor,
        })
    }

    pub(super) async fn find_all<T: From<AccountSet>>(
        &self,
        ids: &[AccountSetId],
    ) -> Result<HashMap<AccountSetId, T>, AccountSetError> {
        let mut tx = self.pool.begin().await?;
        self.find_all_in_tx(&mut tx, ids).await
    }

    pub(super) async fn find_all_in_tx<T: From<AccountSet>>(
        &self,
        db: &mut Transaction<'_, Postgres>,
        ids: &[AccountSetId],
    ) -> Result<HashMap<AccountSetId, T>, AccountSetError> {
        let rows = sqlx::query_as!(
            GenericEvent,
            r#"SELECT s.id, e.sequence, e.event,
                s.created_at AS entity_created_at, e.recorded_at AS event_recorded_at
            FROM cala_accounts s
            JOIN cala_account_set_events e
            ON s.data_source_id = e.data_source_id
            AND s.id = e.id
            WHERE s.data_source_id = '00000000-0000-0000-0000-000000000000'
            AND s.id = ANY($1)
            ORDER BY s.id, e.sequence"#,
            ids as &[AccountSetId]
        )
        .fetch_all(&mut **db)
        .await?;
        let n = rows.len();
        let ret = EntityEvents::load_n(rows, n)?
            .0
            .into_iter()
            .map(|account: AccountSet| (account.values().id, T::from(account)))
            .collect();
        Ok(ret)
    }

    #[cfg(feature = "import")]
    pub async fn import(
        &self,
        db: &mut Transaction<'_, Postgres>,
        recorded_at: DateTime<Utc>,
        origin: DataSourceId,
        account_set: &mut AccountSet,
    ) -> Result<(), AccountSetError> {
        sqlx::query!(
            r#"INSERT INTO cala_account_sets (data_source_id, id, journal_id, name, created_at)
            VALUES ($1, $2, $3, $4, $5)"#,
            origin as DataSourceId,
            account_set.values().id as AccountSetId,
            account_set.values().journal_id as JournalId,
            account_set.values().name,
            recorded_at
        )
        .execute(&mut **db)
        .await?;
        account_set
            .events
            .persisted_at(db, origin, recorded_at)
            .await?;
        Ok(())
    }

    pub async fn fetch_mappings(
        &self,
        journal_id: JournalId,
        account_ids: &[AccountId],
    ) -> Result<HashMap<AccountId, Vec<AccountSetId>>, AccountSetError> {
        let rows = sqlx::query!(
            r#"
          SELECT m.account_set_id AS "set_id!: AccountSetId", m.member_account_id AS "account_id!: AccountId"
          FROM cala_account_set_member_accounts m
          JOIN cala_account_sets s
          ON m.account_set_id = s.id AND s.journal_id = $1
            AND m.data_source_id = s.data_source_id
          WHERE s.data_source_id = '00000000-0000-0000-0000-000000000000'
          AND m.member_account_id = ANY($2)
          "#,
            journal_id as JournalId,
            account_ids as &[AccountId]
        )
        .fetch_all(&self.pool)
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
    pub async fn import_member_account(
        &self,
        db: &mut Transaction<'_, Postgres>,
        recorded_at: DateTime<Utc>,
        origin: DataSourceId,
        account_set_id: AccountSetId,
        account_id: AccountId,
    ) -> Result<(), AccountSetError> {
        sqlx::query!(
            r#"INSERT INTO cala_account_set_member_accounts (data_source_id, account_set_id, member_account_id, created_at)
            VALUES ($1, $2, $3, $4)"#,
            origin as DataSourceId,
            account_set_id as AccountSetId,
            account_id as AccountId,
            recorded_at
        )
        .execute(&mut **db)
        .await?;
        Ok(())
    }

    #[cfg(feature = "import")]
    pub async fn import_remove_member_account(
        &self,
        db: &mut Transaction<'_, Postgres>,
        origin: DataSourceId,
        account_set_id: AccountSetId,
        account_id: AccountId,
    ) -> Result<(), AccountSetError> {
        sqlx::query!(
            r#"DELETE FROM cala_account_set_member_accounts
            WHERE data_source_id = $1 AND account_set_id = $2 AND member_account_id = $3"#,
            origin as DataSourceId,
            account_set_id as AccountSetId,
            account_id as AccountId,
        )
        .execute(&mut **db)
        .await?;
        Ok(())
    }

    #[cfg(feature = "import")]
    pub async fn import_member_set(
        &self,
        db: &mut Transaction<'_, Postgres>,
        recorded_at: DateTime<Utc>,
        origin: DataSourceId,
        account_set_id: AccountSetId,
        member_account_set_id: AccountSetId,
    ) -> Result<(), AccountSetError> {
        sqlx::query!(
            r#"INSERT INTO cala_account_set_member_account_sets (data_source_id, account_set_id, member_account_set_id, created_at)
            VALUES ($1, $2, $3, $4)"#,
            origin as DataSourceId,
            account_set_id as AccountSetId,
            member_account_set_id as AccountSetId,
            recorded_at
        )
        .execute(&mut **db)
        .await?;
        Ok(())
    }

    #[cfg(feature = "import")]
    pub async fn import_remove_member_set(
        &self,
        db: &mut Transaction<'_, Postgres>,
        origin: DataSourceId,
        account_set_id: AccountSetId,
        member_account_set_id: AccountSetId,
    ) -> Result<(), AccountSetError> {
        sqlx::query!(
            r#"DELETE FROM cala_account_set_member_account_sets
            WHERE data_source_id = $1 AND account_set_id = $2 AND member_account_set_id = $3"#,
            origin as DataSourceId,
            account_set_id as AccountSetId,
            member_account_set_id as AccountSetId,
        )
        .execute(&mut **db)
        .await?;
        Ok(())
    }

    #[cfg(feature = "import")]
    pub async fn find_imported(
        &self,
        account_set_id: AccountSetId,
        origin: DataSourceId,
    ) -> Result<AccountSet, AccountSetError> {
        let rows = sqlx::query_as!(
            GenericEvent,
            r#"SELECT a.id, e.sequence, e.event,
                a.created_at AS entity_created_at, e.recorded_at AS event_recorded_at
            FROM cala_account_sets a
            JOIN cala_account_set_events e
            ON a.data_source_id = e.data_source_id
            AND a.id = e.id
            WHERE a.data_source_id = $1
            AND a.id = $2
            ORDER BY e.sequence"#,
            origin as DataSourceId,
            account_set_id as AccountSetId
        )
        .fetch_all(&self.pool)
        .await?;
        match EntityEvents::load_first(rows) {
            Ok(account_set) => Ok(account_set),
            Err(EntityError::NoEntityEventsPresent) => {
                Err(AccountSetError::CouldNotFindById(account_set_id))
            }
            Err(e) => Err(e.into()),
        }
    }

    #[cfg(feature = "import")]
    pub async fn persist_at_in_tx(
        &self,
        db: &mut Transaction<'_, Postgres>,
        recorded_at: DateTime<Utc>,
        origin: DataSourceId,
        account_set: &mut AccountSet,
    ) -> Result<(), AccountSetError> {
        sqlx::query!(
            r#"UPDATE cala_account_sets
            SET name = $3
            WHERE data_source_id = $1 AND id = $2"#,
            origin as DataSourceId,
            account_set.values().id as AccountSetId,
            account_set.values().name,
        )
        .execute(&mut **db)
        .await?;
        account_set
            .events
            .persisted_at(db, origin, recorded_at)
            .await?;
        Ok(())
    }
}
