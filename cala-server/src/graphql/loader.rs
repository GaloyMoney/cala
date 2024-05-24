use async_graphql::dataloader::Loader;

use std::{collections::HashMap, sync::Arc};

use super::{
    account::Account, journal::Journal, transaction::Transaction, tx_template::TxTemplate,
};
use cala_ledger::{
    account::{error::AccountError, *},
    balance::{error::BalanceError, *},
    journal::{error::JournalError, *},
    primitives::*,
    transaction::error::TransactionError,
    tx_template::error::TxTemplateError,
    *,
};

pub struct LedgerDataLoader {
    pub ledger: CalaLedger,
}

impl Loader<BalanceId> for LedgerDataLoader {
    type Value = AccountBalance;
    type Error = Arc<BalanceError>;

    async fn load(
        &self,
        keys: &[BalanceId],
    ) -> Result<HashMap<BalanceId, AccountBalance>, Self::Error> {
        self.ledger
            .balances()
            .find_all(keys)
            .await
            .map_err(Arc::new)
    }
}

impl Loader<AccountId> for LedgerDataLoader {
    type Value = Account;
    type Error = Arc<AccountError>;

    async fn load(&self, keys: &[AccountId]) -> Result<HashMap<AccountId, Account>, Self::Error> {
        self.ledger
            .accounts()
            .find_all(keys)
            .await
            .map_err(Arc::new)
    }
}

impl Loader<JournalId> for LedgerDataLoader {
    type Value = Journal;
    type Error = Arc<JournalError>;

    async fn load(&self, keys: &[JournalId]) -> Result<HashMap<JournalId, Journal>, Self::Error> {
        self.ledger
            .journals()
            .find_all(keys)
            .await
            .map_err(Arc::new)
    }
}

impl Loader<TransactionId> for LedgerDataLoader {
    type Value = Transaction;
    type Error = Arc<TransactionError>;

    async fn load(
        &self,
        keys: &[TransactionId],
    ) -> Result<HashMap<TransactionId, Transaction>, Self::Error> {
        self.ledger
            .transactions()
            .find_all(keys)
            .await
            .map_err(Arc::new)
    }
}

impl Loader<TxTemplateId> for LedgerDataLoader {
    type Value = TxTemplate;
    type Error = Arc<TxTemplateError>;

    async fn load(
        &self,
        keys: &[TxTemplateId],
    ) -> Result<HashMap<TxTemplateId, TxTemplate>, Self::Error> {
        self.ledger
            .tx_templates()
            .find_all(keys)
            .await
            .map_err(Arc::new)
    }
}
