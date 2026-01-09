#![no_main]

use libfuzzer_sys::fuzz_target;
use serde_json::json;

use cala_ledger::es_entity::TryFromEvents;
use cala_ledger::{transaction::*, *};

/* simple example to show how to get started with cargo fuzz: create transaction
 * with random ascii values (pass --only_ascii=1 to cargo fuzz run) */

fuzz_target!(|data: &[u8]| {
    struct FuzzData {
        data: Vec<String>,
        i: usize,
        m: usize,
    }

    impl FuzzData {
        fn new(bytes: &[u8], nr_chunks: usize) -> Self {
            FuzzData {
                data: bytes
                    .chunks(nr_chunks)
                    .map(|c| std::str::from_utf8(c).unwrap_or("").to_string())
                    .collect(),
                i: 0,
                m: nr_chunks,
            }
        }

        fn next(&mut self) -> String {
            assert!(self.i < self.m);
            self.i += 1;
            self.data[self.i - 1].clone()
        }
    }

    fn transaction(data: &[u8]) -> Transaction {
        // split fuzzed data into chunks and use it for transaction fields
        let mut fuzz_data: FuzzData = FuzzData::new(data, 16);
        let id = TransactionId::new();
        let values = TransactionValues {
            id,
            version: 1,
            created_at: chrono::Utc::now(),
            modified_at: chrono::Utc::now(),
            journal_id: JournalId::new(),
            tx_template_id: TxTemplateId::new(),
            entry_ids: vec![],
            effective: chrono::Utc::now().date_naive(),
            correlation_id: (&fuzz_data.next()).to_string(),
            external_id: Some(fuzz_data.next().to_string()),
            description: Some(fuzz_data.next().to_string()),
            voided_by: None,
            void_of: None,
            metadata: Some(serde_json::json!({
                "tx": fuzz_data.next(),
                fuzz_data.next(): true,
                fuzz_data.next(): false,
                fuzz_data.next(): fuzz_data.next(),
                "test": fuzz_data.next()
            })),
        };

        let events = es_entity::EntityEvents::init(id, [TransactionEvent::Initialized { values }]);
        Transaction::try_from_events(events).unwrap()
    }

    if data.len() < 512 {
        return;
    }

    let transaction = transaction(data);

    // call some getters
    transaction.values();
    transaction.journal_id();
    transaction.effective();
});
