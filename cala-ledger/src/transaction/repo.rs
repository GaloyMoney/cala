use es_entity::*;
use sqlx::PgPool;

use crate::primitives::{JournalId, TransactionId, TxTemplateId};

use super::entity::*;

#[derive(EsRepo, Clone)]
#[es_repo(
    entity = "Transaction",
    columns(
        external_id(ty = "Option<String>", update(persist = false)),
        // No `find_by`: nothing looks a transaction up by correlation id, and
        // the index that would back it was dropped from the migration.
        correlation_id(ty = "String", update(persist = false), find_by = false),
        journal_id(ty = "JournalId", update(persist = false)),
        tx_template_id(ty = "TxTemplateId", update(persist = false), list_for(by(created_at))),
        effective(ty = "chrono::NaiveDate", update(persist = false)),
    ),
    tbl_prefix = "cala",
    persist_event_context = false
)]
/// Read side of the transaction entity.
///
/// Transactions are written exclusively by [`crate::posting`], which inserts
/// the row and its event in the same statement as the entries and balances, and
/// publishes `TransactionCreated` itself — so this repo carries no persist hook.
pub(super) struct TransactionRepo {
    pool: PgPool,
}

impl TransactionRepo {
    pub fn new(pool: &PgPool) -> Self {
        Self { pool: pool.clone() }
    }
}
