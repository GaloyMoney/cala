use std::future::Future;

use super::BalanceSnapshot;
use crate::primitives::*;

pub struct JournalInfo {
    pub id: JournalId,
    pub is_locked: bool,
    pub enable_effective_balances: bool,
}

pub trait JournalChecker: Clone + Send + Sync + 'static {
    type Error: std::error::Error + Send + Sync + 'static;
    fn check_journal(
        &self,
        journal_id: JournalId,
    ) -> impl Future<Output = Result<JournalInfo, Self::Error>> + Send;
}

pub trait BalanceProvider: Clone + Send + Sync + 'static {
    type Error: std::error::Error + Send + Sync + 'static;
    fn find_balances_for_update(
        &self,
        db: &mut impl es_entity::AtomicOperation,
        journal_id: JournalId,
        account_id: AccountId,
    ) -> impl Future<
        Output = Result<std::collections::HashMap<Currency, BalanceSnapshot>, Self::Error>,
    > + Send;
    fn update_balances_in_op(
        &self,
        db: &mut impl es_entity::AtomicOperation,
        journal_id: JournalId,
        entries: Vec<crate::entry::EntryValues>,
        effective: chrono::NaiveDate,
        created_at: chrono::DateTime<chrono::Utc>,
        account_set_mappings: std::collections::HashMap<AccountId, Vec<AccountSetId>>,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send;
}
