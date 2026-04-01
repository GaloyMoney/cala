-- Add a globally-ordered sequence column to cala_balance_history.
-- Replaces UUID v7 entry_id as the watermark for incremental EC recalculation,
-- which is unsafe in multi-writer (HA) deployments due to clock drift.

CREATE SEQUENCE cala_balance_history_seq_seq AS BIGINT;

ALTER TABLE cala_balance_history
    ADD COLUMN seq BIGINT NOT NULL DEFAULT nextval('cala_balance_history_seq_seq');

ALTER SEQUENCE cala_balance_history_seq_seq OWNED BY cala_balance_history.seq;

CREATE INDEX idx_cala_balance_history_seq ON cala_balance_history (seq);

-- Track the high-water seq on cala_current_balances for efficient watermark lookup.
ALTER TABLE cala_current_balances
    ADD COLUMN latest_seq BIGINT NOT NULL DEFAULT 0;

-- Backfill latest_seq from existing balance_history rows.
UPDATE cala_current_balances cb
SET latest_seq = COALESCE(
    (SELECT MAX(bh.seq)
     FROM cala_balance_history bh
     WHERE bh.journal_id = cb.journal_id
       AND bh.account_id = cb.account_id
       AND bh.currency = cb.currency), 0);
