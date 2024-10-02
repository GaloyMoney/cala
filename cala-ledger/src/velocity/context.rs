use std::collections::HashMap;

use cala_types::{entry::EntryValues, transaction::TransactionValues};
use cel_interpreter::{CelMap, CelValue};

use crate::{cel_context::*, primitives::EntryId};

pub struct EvalContext {
    transaction: CelValue,
    entry_values: HashMap<EntryId, CelValue>,
}

impl EvalContext {
    pub fn new(transaction: &TransactionValues) -> Self {
        Self {
            transaction: transaction.into(),
            entry_values: HashMap::new(),
        }
    }

    pub fn control_context(&mut self, entry: &EntryValues) -> CelContext {
        let entry = self
            .entry_values
            .entry(entry.id)
            .or_insert_with(|| entry.into());

        let mut vars = CelMap::new();
        vars.insert("transaction", self.transaction.clone());
        vars.insert("entry", entry.clone());

        let mut context = CelMap::new();
        context.insert("vars", vars);

        let mut ctx = initialize();
        ctx.add_variable("context", context);

        ctx
    }
}
