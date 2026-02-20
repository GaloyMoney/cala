use std::future::Future;

use crate::{entry::EntryValues, primitives::*, velocity::VelocityContextAccountValues};

pub struct NewAccountParams {
    pub id: AccountId,
    pub code: String,
    pub name: String,
    pub normal_balance_type: DebitOrCredit,
    pub is_account_set: bool,
    pub velocity_context_values: Option<VelocityContextAccountValues>,
}

pub struct NewEntryParams {
    pub id: EntryId,
    pub transaction_id: TransactionId,
    pub journal_id: JournalId,
    pub account_id: AccountId,
    pub entry_type: String,
    pub sequence: u32,
    pub layer: Layer,
    pub units: rust_decimal::Decimal,
    pub currency: Currency,
    pub direction: DebitOrCredit,
    pub description: Option<String>,
    pub metadata: Option<serde_json::Value>,
}

pub trait AccountCreator: Clone + Send + Sync + 'static {
    type Error: std::error::Error + Send + Sync + 'static;
    fn create_in_op(
        &self,
        db: &mut impl es_entity::AtomicOperation,
        params: NewAccountParams,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send;
    fn create_all_in_op(
        &self,
        db: &mut impl es_entity::AtomicOperation,
        params: Vec<NewAccountParams>,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send;
    fn update_velocity_context_values_in_op(
        &self,
        db: &mut impl es_entity::AtomicOperation,
        values: VelocityContextAccountValues,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send;
}

pub trait EntryCreator: Clone + Send + Sync + 'static {
    type Error: std::error::Error + Send + Sync + 'static;
    fn create_all_in_op(
        &self,
        db: &mut impl es_entity::AtomicOperation,
        entries: Vec<NewEntryParams>,
    ) -> impl Future<Output = Result<Vec<EntryValues>, Self::Error>> + Send;
}
