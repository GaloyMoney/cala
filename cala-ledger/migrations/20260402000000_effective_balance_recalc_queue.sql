CREATE TABLE cala_effective_balance_recalc_queue (
    journal_id UUID NOT NULL,
    account_id UUID NOT NULL,
    currency VARCHAR NOT NULL,
    earliest_effective_date DATE NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (journal_id, account_id, currency)
);
