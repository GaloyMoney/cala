CREATE TABLE cala_calculations (
  id UUID PRIMARY KEY,
  journal_id UUID NOT NULL REFERENCES cala_journals(id),
  code VARCHAR NOT NULL UNIQUE,
  name VARCHAR NOT NULL,
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX idx_cala_calculations_journal_id ON cala_calculations (journal_id);

CREATE TABLE cala_calculation_events (
  id UUID NOT NULL REFERENCES cala_calculations(id),
  sequence INT NOT NULL,
  event_type VARCHAR NOT NULL,
  event JSONB NOT NULL,
  context JSONB DEFAULT NULL,
  recorded_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  UNIQUE(id, sequence)
);

CREATE TABLE cala_calculation_attachments (
  calculation_id UUID NOT NULL REFERENCES cala_calculations(id),
  account_id UUID NOT NULL REFERENCES cala_accounts(id),
  activation_sequence BIGINT NOT NULL,
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  UNIQUE(calculation_id, account_id)
);
CREATE INDEX idx_cala_calculation_attachments_account_id
  ON cala_calculation_attachments (account_id);

-- Same columns as cala_cumulative_effective_balances plus the two series key columns.
-- One row per (series, effective date) holding the cumulative balance at end of that date.
CREATE TABLE cala_calculation_effective_balances (
  journal_id UUID NOT NULL,
  calculation_id UUID NOT NULL REFERENCES cala_calculations(id),
  account_id UUID NOT NULL,
  currency VARCHAR NOT NULL,
  dimensions JSONB NOT NULL,
  effective DATE NOT NULL,
  version INT NOT NULL,
  all_time_version INT NOT NULL,
  latest_entry_id UUID NOT NULL,
  settled_dr_balance NUMERIC NOT NULL,
  settled_cr_balance NUMERIC NOT NULL,
  settled_entry_id UUID NOT NULL,
  settled_modified_at TIMESTAMPTZ NOT NULL,
  pending_dr_balance NUMERIC NOT NULL,
  pending_cr_balance NUMERIC NOT NULL,
  pending_entry_id UUID NOT NULL,
  pending_modified_at TIMESTAMPTZ NOT NULL,
  encumbrance_dr_balance NUMERIC NOT NULL,
  encumbrance_cr_balance NUMERIC NOT NULL,
  encumbrance_entry_id UUID NOT NULL,
  encumbrance_modified_at TIMESTAMPTZ NOT NULL,
  updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  PRIMARY KEY (journal_id, calculation_id, account_id, currency, dimensions, effective)
);
-- For Phase E's list_modified_since; narrow daily windows, same reasoning as the base index.
CREATE INDEX idx_cala_calculation_effective_balances_updated_at
  ON cala_calculation_effective_balances (journal_id, calculation_id, updated_at);
