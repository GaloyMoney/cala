use async_graphql::dataloader::Loader;

use std::{collections::HashMap, sync::Arc};

use cala_ledger::{
    account::{error::AccountError, *},
    balance::{error::BalanceError, *},
    primitives::*,
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
    type Value = AccountValues;
    type Error = Arc<AccountError>;

    async fn load(
        &self,
        keys: &[AccountId],
    ) -> Result<HashMap<AccountId, AccountValues>, Self::Error> {
        self.ledger
            .accounts()
            .find_all(keys)
            .await
            .map_err(Arc::new)
    }
}
