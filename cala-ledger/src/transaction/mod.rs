pub mod error;

mod entity;
mod repo;
use chrono::{DateTime, Utc};
use cala_types::primitives::AccountId;
use es_entity::EsEntity;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use tracing::instrument;

use std::collections::HashMap;

#[cfg(feature = "import")]
use crate::primitives::DataSourceId;
use crate::primitives::TxTemplateId;
use crate::{ledger_operation::*, outbox::*, primitives::DataSource};

pub use entity::*;
use error::*;
pub use repo::transaction_cursor::TransactionsByCreatedAtCursor;
use repo::*;

use async_graphql::connection::CursorType;

#[derive(Clone)]
pub struct Transactions {
    repo: TransactionRepo,
    outbox: Outbox,
    _pool: PgPool,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransactionsByAccountIdCursor {
    pub query_account_id: AccountId,
    pub created_at: DateTime<Utc>,
    pub transaction_id: TransactionId,
}

impl Transactions {
    pub(crate) fn new(pool: &PgPool, outbox: Outbox) -> Self {
        Self {
            repo: TransactionRepo::new(pool),
            outbox,
            _pool: pool.clone(),
        }
    }

    pub(crate) async fn create_in_op(
        &self,
        db: &mut LedgerOperation<'_>,
        new_transaction: NewTransaction,
    ) -> Result<Transaction, TransactionError> {
        let transaction = self.repo.create_in_op(db.op(), new_transaction).await?;
        db.accumulate(transaction.last_persisted(1).map(|p| &p.event));
        Ok(transaction)
    }

    #[instrument(name = "cala_ledger.transactions.find_by_external_id", skip(self), err)]
    pub async fn find_by_external_id(
        &self,
        external_id: String,
    ) -> Result<Transaction, TransactionError> {
        self.repo.find_by_external_id(Some(external_id)).await
    }

    #[instrument(name = "cala_ledger.transactions.find_by_id", skip(self), err)]
    pub async fn find_by_id(
        &self,
        transaction_id: TransactionId,
    ) -> Result<Transaction, TransactionError> {
        self.repo.find_by_id(transaction_id).await
    }

    #[instrument(
        name = "cala_ledger.transactions.list_for_template_id",
        skip(self),
        err
    )]
    pub async fn list_for_template_id(
        &self,
        template_id: TxTemplateId,
        query: es_entity::PaginatedQueryArgs<TransactionsByCreatedAtCursor>,
        direction: es_entity::ListDirection,
    ) -> Result<
        es_entity::PaginatedQueryRet<Transaction, TransactionsByCreatedAtCursor>,
        TransactionError,
    > {
        self.repo
            .list_for_tx_template_id_by_created_at(template_id, query, direction)
            .await
    }

    #[instrument(name = "cala_ledger.transactions.find_all", skip(self), err)]
    pub async fn find_all<T: From<Transaction>>(
        &self,
        transaction_ids: &[TransactionId],
    ) -> Result<HashMap<TransactionId, T>, TransactionError> {
        self.repo.find_all(transaction_ids).await
    }
    
    #[instrument(name = "cala_ledger.transactions.find_by_account_id", skip(self), err)]
    pub async fn find_by_account_id(
        &self,
        account_id: AccountId,
        args: es_entity::PaginatedQueryArgs<TransactionsByAccountIdCursor>,
    ) -> Result<es_entity::PaginatedQueryRet<Transaction, TransactionsByAccountIdCursor>, TransactionError> {
        self.repo.find_by_account_id(account_id, args).await
    }

    #[cfg(feature = "import")]
    pub async fn sync_transaction_creation(
        &self,
        mut db: es_entity::DbOp<'_>,
        origin: DataSourceId,
        values: TransactionValues,
    ) -> Result<(), TransactionError> {
        let mut transaction = Transaction::import(origin, values);
        self.repo
            .import_in_op(&mut db, origin, &mut transaction)
            .await?;
        let recorded_at = db.now();
        let outbox_events: Vec<_> = transaction
            .last_persisted(1)
            .map(|p| OutboxEventPayload::from(&p.event))
            .collect();
        self.outbox
            .persist_events_at(db.into_tx(), outbox_events, recorded_at)
            .await?;
        Ok(())
    }
}

impl From<&TransactionEvent> for OutboxEventPayload {
    fn from(event: &TransactionEvent) -> Self {
        match event {
            #[cfg(feature = "import")]
            TransactionEvent::Imported {
                source,
                values: transaction,
            } => OutboxEventPayload::TransactionCreated {
                source: *source,
                transaction: transaction.clone(),
            },
            TransactionEvent::Initialized {
                values: transaction,
            } => OutboxEventPayload::TransactionCreated {
                source: DataSource::Local,
                transaction: transaction.clone(),
            },
        }
    }
}


impl From<(&Transaction, AccountId)> for TransactionsByAccountIdCursor {
    fn from((transaction, account_id): (&Transaction, AccountId)) -> Self {
        Self {
            query_account_id: account_id,
            created_at: transaction.created_at(),
            transaction_id: transaction.id(),
        }
    }
}

// Implement CursorType trait required by async-graphql's Connection..
impl CursorType for TransactionsByAccountIdCursor {
    // Use String as Error type since it implements Display
    type Error = String;

    fn decode_cursor(s: &str) -> Result<Self, Self::Error> {
        // Decode a cursor from a string representation
        // Format: <account_id>:<created_at_timestamp>:<transaction_id>
        let parts: Vec<&str> = s.split(':').collect();
        if parts.len() != 3 {
            return Err(format!("Invalid cursor format: {}", s));
        }
        
        let account_id = parts[0].parse::<AccountId>()
            .map_err(|e| format!("Invalid account ID in cursor: {}", e))?;
            
        let created_at_nanos = parts[1].parse::<i64>()
            .map_err(|e| format!("Invalid timestamp in cursor: {}", e))?;
        
        // Use non-deprecated functions from chrono
        let seconds = created_at_nanos / 1_000_000_000;
        let nanoseconds = (created_at_nanos % 1_000_000_000) as u32;
        let datetime = chrono::DateTime::from_timestamp(seconds, nanoseconds)
            .ok_or_else(|| format!("Invalid timestamp value: {}", created_at_nanos))?;
        let created_at = datetime;
        
        let transaction_id = parts[2].parse::<TransactionId>()
            .map_err(|e| format!("Invalid transaction ID in cursor: {}", e))?;
            
        Ok(Self {
            query_account_id: account_id,
            created_at,
            transaction_id,
        })
    }

    fn encode_cursor(&self) -> String {
        // Encode the cursor as a string
        // Format: <account_id>:<created_at_timestamp>:<transaction_id>
        // Use non-deprecated function
        let nanos = self.created_at.timestamp_nanos_opt()
            .unwrap_or(0);
            
        format!(
            "{}:{}:{}",
            self.query_account_id,
            nanos,
            self.transaction_id
        )
    }
}
