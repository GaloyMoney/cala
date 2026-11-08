use chrono::{DateTime, Utc};
use es_entity::{
    errlanes::{lanes, Fail, ResultExt},
    Ensured,
};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use std::collections::HashMap;

use crate::{error::CalaFault, primitives::*};

use super::error::{AttachWrite, CalculationAlreadyAttached};

/// An account or account set a calculation maintains series for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CalculationAttachment {
    pub calculation_id: CalculationId,
    pub account_id: AccountId,
    pub activation_sequence: u64,
    /// `false` while the attachment's history is being folded in; the tip
    /// ignores the attachment until then.
    pub backfill_complete: bool,
    pub created_at: DateTime<Utc>,
}

/// What the rollup needs to decide whether an attachment covers a transaction.
#[derive(Debug, Clone, Copy)]
pub(super) struct AttachmentCoverage {
    pub activation_sequence: u64,
    pub backfill_complete: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttachmentByAccountIdCursor {
    pub account_id: AccountId,
}

impl From<&CalculationAttachment> for AttachmentByAccountIdCursor {
    fn from(attachment: &CalculationAttachment) -> Self {
        Self {
            account_id: attachment.account_id,
        }
    }
}

/// One row for [`AttachmentRepo::create_all_in_op`].
#[derive(Debug, Clone, Copy)]
pub(super) struct NewAttachment {
    pub account_id: AccountId,
    pub backfill_complete: bool,
}

/// What an attach target resolves to: whether its account row exists and,
/// when it is an account set, the set's journal.
pub(super) struct AttachTarget {
    pub account_id: AccountId,
    pub exists: bool,
    pub set_journal_id: Option<JournalId>,
}

#[derive(Debug, Clone)]
pub(super) struct AttachmentRepo {
    pool: PgPool,
}

impl AttachmentRepo {
    pub fn new(pool: &PgPool) -> Self {
        Self { pool: pool.clone() }
    }

    #[es_entity::errlanes::instrument(
        level = "debug",
        name = "calculation_attachment.resolve_targets",
        skip_all,
        fields(count = account_ids.len())
    )]
    pub async fn resolve_targets(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        account_ids: &[AccountId],
    ) -> Result<Vec<AttachTarget>, CalaFault> {
        let rows = sqlx::query!(
            r#"
            SELECT t.id AS "account_id!: AccountId",
                   (a.id IS NOT NULL) AS "exists!",
                   s.journal_id AS "set_journal_id?: JournalId"
            FROM UNNEST($1::uuid[]) AS t(id)
            LEFT JOIN cala_accounts a ON a.id = t.id
            LEFT JOIN cala_account_sets s ON s.id = t.id
            "#,
            account_ids as &[AccountId],
        )
        .fetch_all(op.as_executor())
        .await?;
        Ok(rows
            .into_iter()
            .map(|row| AttachTarget {
                account_id: row.account_id,
                exists: row.exists,
                set_journal_id: row.set_journal_id,
            })
            .collect())
    }

    /// One multi-row `INSERT` for every target, sorted by account id so
    /// that concurrent callers insert overlapping rows in the same order.
    #[es_entity::errlanes::instrument(
        level = "debug",
        name = "calculation_attachment.create_all_in_op",
        skip_all,
        fields(count = targets.len())
    )]
    pub async fn create_all_in_op(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        calculation_id: CalculationId,
        targets: &[NewAttachment],
        activation_sequence: u64,
    ) -> Result<(), Fail<CalculationAlreadyAttached, lanes!(Transient, Fatal)>> {
        if targets.is_empty() {
            return Ok(());
        }
        let mut targets = targets.to_vec();
        targets.sort_unstable_by_key(|t| t.account_id);
        let (account_ids, complete): (Vec<AccountId>, Vec<bool>) = targets
            .into_iter()
            .map(|t| (t.account_id, t.backfill_complete))
            .unzip();

        sqlx::query!(
            r#"
            INSERT INTO cala_calculation_attachments
              (calculation_id, account_id, activation_sequence, backfill_complete)
            SELECT $1, account_id, $3, backfill_complete
            FROM UNNEST($2::uuid[], $4::bool[]) AS t(account_id, backfill_complete)
            "#,
            calculation_id as CalculationId,
            &account_ids as &[AccountId],
            activation_sequence as i64,
            &complete,
        )
        .execute(op.as_executor())
        .await
        .classify::<AttachWrite>()?;
        Ok(())
    }

    #[es_entity::errlanes::instrument(
        level = "debug",
        name = "calculation_attachment.ensure_in_op",
        skip_all
    )]
    pub async fn ensure_in_op(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        calculation_id: CalculationId,
        account_id: AccountId,
        activation_sequence: u64,
        backfill_complete: bool,
    ) -> Result<Ensured<()>, CalaFault> {
        let inserted = sqlx::query!(
            r#"
            INSERT INTO cala_calculation_attachments
              (calculation_id, account_id, activation_sequence, backfill_complete)
            VALUES ($1, $2, $3, $4)
            ON CONFLICT (calculation_id, account_id) DO NOTHING
            "#,
            calculation_id as CalculationId,
            account_id as AccountId,
            activation_sequence as i64,
            backfill_complete,
        )
        .execute(op.as_executor())
        .await?
        .rows_affected();
        Ok(if inserted == 0 {
            Ensured::Found(())
        } else {
            Ensured::Created(())
        })
    }

    #[es_entity::errlanes::instrument(
        level = "debug",
        name = "calculation_attachment.list_for_calculation",
        skip_all
    )]
    pub async fn list_for_calculation(
        &self,
        calculation_id: CalculationId,
        args: es_entity::PaginatedQueryArgs<AttachmentByAccountIdCursor>,
    ) -> Result<
        es_entity::PaginatedQueryRet<CalculationAttachment, AttachmentByAccountIdCursor>,
        CalaFault,
    > {
        let es_entity::PaginatedQueryArgs { first, after } = args;
        let after = after.map(|cursor| cursor.account_id);
        let rows = sqlx::query!(
            r#"
            SELECT calculation_id AS "calculation_id: CalculationId",
                   account_id AS "account_id: AccountId",
                   activation_sequence,
                   backfill_complete,
                   created_at
            FROM cala_calculation_attachments
            WHERE calculation_id = $1
              AND ($2::uuid IS NULL OR account_id > $2)
            ORDER BY account_id
            LIMIT $3
            "#,
            calculation_id as CalculationId,
            after as Option<AccountId>,
            (first + 1) as i64,
        )
        .fetch_all(&self.pool)
        .await?;

        let has_next_page = rows.len() > first;
        let entities: Vec<_> = rows
            .into_iter()
            .take(first)
            .map(|row| CalculationAttachment {
                calculation_id: row.calculation_id,
                account_id: row.account_id,
                activation_sequence: row.activation_sequence as u64,
                backfill_complete: row.backfill_complete,
                created_at: row.created_at,
            })
            .collect();
        let end_cursor = entities.last().map(AttachmentByAccountIdCursor::from);
        Ok(es_entity::PaginatedQueryRet::new(
            entities,
            has_next_page,
            end_cursor,
            first,
        ))
    }

    /// `backfill_complete` of one attachment, `None` when not attached.
    #[es_entity::errlanes::instrument(
        level = "debug",
        name = "calculation_attachment.backfill_complete",
        skip_all
    )]
    pub async fn backfill_complete(
        &self,
        calculation_id: CalculationId,
        account_id: AccountId,
    ) -> Result<Option<bool>, CalaFault> {
        let complete = sqlx::query_scalar!(
            r#"
            SELECT backfill_complete
            FROM cala_calculation_attachments
            WHERE calculation_id = $1 AND account_id = $2
            "#,
            calculation_id as CalculationId,
            account_id as AccountId,
        )
        .fetch_optional(&self.pool)
        .await?;
        Ok(complete)
    }

    /// A catch-up's join: every target becomes `Complete` with
    /// `activation_sequence` set to the sequence it joined the tip at.
    #[es_entity::errlanes::instrument(
        level = "debug",
        name = "calculation_attachment.complete_backfill_in_op",
        skip_all,
        fields(count = account_ids.len(), activation_sequence)
    )]
    pub async fn complete_backfill_in_op(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        calculation_id: CalculationId,
        account_ids: &[AccountId],
        activation_sequence: u64,
    ) -> Result<(), CalaFault> {
        sqlx::query!(
            r#"
            UPDATE cala_calculation_attachments
            SET backfill_complete = TRUE, activation_sequence = $3
            WHERE calculation_id = $1 AND account_id = ANY($2)
            "#,
            calculation_id as CalculationId,
            account_ids as &[AccountId],
            activation_sequence as i64,
        )
        .execute(op.as_executor())
        .await?;
        Ok(())
    }

    /// Every attachment of `calculation_ids` to one of `account_ids`, as
    /// `(calculation, target) -> coverage`.
    #[es_entity::errlanes::instrument(
        level = "debug",
        name = "calculation_attachment.find_for_accounts",
        skip_all,
        fields(calculations = calculation_ids.len(), accounts = account_ids.len())
    )]
    pub async fn find_for_accounts(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        calculation_ids: &[CalculationId],
        account_ids: &[AccountId],
    ) -> Result<HashMap<(CalculationId, AccountId), AttachmentCoverage>, CalaFault> {
        if calculation_ids.is_empty() || account_ids.is_empty() {
            return Ok(HashMap::new());
        }
        let rows = sqlx::query!(
            r#"
            SELECT calculation_id AS "calculation_id: CalculationId",
                   account_id AS "account_id: AccountId",
                   activation_sequence,
                   backfill_complete
            FROM cala_calculation_attachments
            WHERE calculation_id = ANY($1)
              AND account_id = ANY($2)
            "#,
            calculation_ids as &[CalculationId],
            account_ids as &[AccountId],
        )
        .fetch_all(op.as_executor())
        .await?;
        Ok(rows
            .into_iter()
            .map(|row| {
                (
                    (row.calculation_id, row.account_id),
                    AttachmentCoverage {
                        activation_sequence: row.activation_sequence as u64,
                        backfill_complete: row.backfill_complete,
                    },
                )
            })
            .collect())
    }
}
