use std::collections::HashMap;

use cala_types::{account::AccountValues, entry::EntryValues, transaction::TransactionValues};
use cel_interpreter::{CelMap, CelValue};

use crate::{
    cel_context::*,
    primitives::{AccountId, EntryId},
};

pub struct EvalContext {
    transaction: CelValue,
    entry_values: HashMap<EntryId, CelValue>,
    account_values: HashMap<AccountId, CelValue>,
}

impl EvalContext {
    pub fn new<'a>(
        transaction: &TransactionValues,
        accounts: impl Iterator<Item = &'a AccountValues>,
    ) -> Self {
        let account_values = accounts.map(|a| (a.id, a.into())).collect();
        Self {
            transaction: transaction.into(),
            entry_values: HashMap::new(),
            account_values,
        }
    }

    pub fn context_for_entry(&mut self, entry: &EntryValues) -> CelContext {
        let cel_entry = self
            .entry_values
            .entry(entry.id)
            .or_insert_with(|| entry.into());

        let mut vars = CelMap::new();
        vars.insert("transaction", self.transaction.clone());
        vars.insert("entry", cel_entry.clone());
        vars.insert(
            "account",
            self.account_values
                .get(&entry.account_id)
                .expect("account values not set for context")
                .clone(),
        );

        let mut context = CelMap::new();
        context.insert("vars", vars);

        let mut ctx = initialize();
        ctx.add_variable("context", context);

        ctx
    }
}
