use async_graphql::dataloader::Loader;

use std::{collections::HashMap, sync::Arc};

use cala_ledger::{
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
