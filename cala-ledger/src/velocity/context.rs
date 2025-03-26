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

#[cfg(test)]
mod tests {
    use rust_decimal::Decimal;
    use serde_json::json;

    use cala_types::account::AccountConfig;
    use cel_interpreter::CelExpression;

    use crate::{primitives::*, velocity::context::EvalContext};

    use super::*;

    fn transaction() -> TransactionValues {
        TransactionValues {
            id: TransactionId::new(),
            version: 1,
            created_at: chrono::Utc::now(),
            modified_at: chrono::Utc::now(),
            journal_id: JournalId::new(),
            tx_template_id: TxTemplateId::new(),
            entry_ids: vec![],
            effective: chrono::Utc::now().date_naive(),
            correlation_id: "correlation_id".to_string(),
            external_id: Some("external_id".to_string()),
            description: None,
            metadata: Some(serde_json::json!({
                "tx": "metadata",
                "test": true,
            })),
        }
    }

    fn account() -> AccountValues {
        AccountValues {
            id: AccountId::new(),
            version: 1,
            code: "code".to_string(),
            name: "name".to_string(),
            normal_balance_type: DebitOrCredit::Credit,
            status: Status::Active,
            external_id: None,
            description: None,
            metadata: Some(json!({
                "account": "metadata",
                "test": true,
            })),
            config: AccountConfig {
                is_account_set: false,
                eventually_consistent: false,
            },
        }
    }

    fn entry(account_id: AccountId, tx: &TransactionValues) -> EntryValues {
        EntryValues {
            id: EntryId::new(),
            version: 1,
            transaction_id: tx.id,
            journal_id: tx.journal_id,
            account_id,
            entry_type: "TEST".to_string(),
            sequence: 1,
            layer: Layer::Settled,
            currency: "USD".parse().unwrap(),
            direction: DebitOrCredit::Credit,
            units: Decimal::from(100),
            description: None,
            metadata: None,
        }
    }

    #[test]
    fn context_for_entry() {
        let account = account();
        let tx = transaction();
        let entry = entry(account.id, &tx);
        let mut context = EvalContext::new(&tx, std::iter::once(&account));
        let ctx = context.context_for_entry(&entry);

        let expr: CelExpression = "context.vars.transaction.id".parse().unwrap();
        let result: uuid::Uuid = expr.try_evaluate(&ctx).unwrap();
        assert!(result == uuid::Uuid::from(tx.id));

        let expr: CelExpression = "context.vars.account.metadata.test".parse().unwrap();
        let result: bool = expr.try_evaluate(&ctx).unwrap();
        assert!(result);

        let expr: CelExpression = "context.vars.entry.units == decimal('100')"
            .parse()
            .unwrap();
        let result: bool = expr.try_evaluate(&ctx).unwrap();
        assert!(result);
    }
}
