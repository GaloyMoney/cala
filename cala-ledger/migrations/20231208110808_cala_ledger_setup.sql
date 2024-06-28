CREATE TYPE DebitOrCredit AS ENUM ('debit', 'credit');
CREATE TYPE Status AS ENUM ('active', 'locked');
CREATE TYPE Layer AS ENUM ('settled', 'pending', 'encumbrance');

CREATE TABLE cala_accounts (
  data_source_id UUID NOT NULL DEFAULT '00000000-0000-0000-0000-000000000000',
  id UUID NOT NULL,
  code VARCHAR NOT NULL,
  name VARCHAR NOT NULL,
  external_id VARCHAR,
  normal_balance_type DebitOrCredit NOT NULL, -- For quick lookup when querying balances
  eventually_consistent BOOLEAN NOT NULL, -- For balance locking
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  UNIQUE(data_source_id, id),
  UNIQUE(data_source_id, code)
);
CREATE INDEX idx_cala_accounts_name ON cala_accounts (name);
CREATE UNIQUE INDEX idx_cala_accounts_data_source_id_external_id ON cala_accounts (data_source_id, external_id) WHERE external_id IS NOT NULL;


CREATE TABLE cala_account_events (
  data_source_id UUID NOT NULL DEFAULT '00000000-0000-0000-0000-000000000000',
  id UUID NOT NULL,
  sequence INT NOT NULL,
  event_type VARCHAR NOT NULL,
  event JSONB NOT NULL,
  recorded_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  UNIQUE(data_source_id, id, sequence),
  FOREIGN KEY (data_source_id, id) REFERENCES cala_accounts(data_source_id, id)
);

CREATE TABLE cala_journals (
  data_source_id UUID NOT NULL DEFAULT '00000000-0000-0000-0000-000000000000',
  id UUID NOT NULL,
  name VARCHAR NOT NULL,
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  UNIQUE(data_source_id, id)
);
CREATE INDEX idx_cala_journals_name ON cala_journals (name);

CREATE TABLE cala_journal_events (
  data_source_id UUID NOT NULL DEFAULT '00000000-0000-0000-0000-000000000000',
  id UUID NOT NULL,
  sequence INT NOT NULL,
  event_type VARCHAR NOT NULL,
  event JSONB NOT NULL,
  recorded_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  UNIQUE(data_source_id, id, sequence),
  FOREIGN KEY (data_source_id, id) REFERENCES cala_journals(data_source_id, id)
);

CREATE TABLE cala_account_sets (
  data_source_id UUID NOT NULL DEFAULT '00000000-0000-0000-0000-000000000000',
  id UUID NOT NULL,
  journal_id UUID NOT NULL,
  name VARCHAR NOT NULL,
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  UNIQUE(data_source_id, id),
  FOREIGN KEY (data_source_id, journal_id) REFERENCES cala_journals(data_source_id, id),
  FOREIGN KEY (data_source_id, id) REFERENCES cala_accounts(data_source_id, id)
);
CREATE INDEX idx_cala_account_sets_name ON cala_account_sets (name);


CREATE TABLE cala_account_set_events (
  data_source_id UUID NOT NULL DEFAULT '00000000-0000-0000-0000-000000000000',
  id UUID NOT NULL,
  sequence INT NOT NULL,
  event_type VARCHAR NOT NULL,
  event JSONB NOT NULL,
  recorded_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  UNIQUE(data_source_id, id, sequence),
  FOREIGN KEY (data_source_id, id) REFERENCES cala_accounts(data_source_id, id),
  FOREIGN KEY (data_source_id, id) REFERENCES cala_account_sets(data_source_id, id)
);

CREATE TABLE cala_account_set_member_accounts (
  data_source_id UUID NOT NULL DEFAULT '00000000-0000-0000-0000-000000000000',
  account_set_id UUID NOT NULL,
  member_account_id UUID NOT NULL,
  transitive BOOLEAN NOT NULL DEFAULT FALSE,
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  UNIQUE(data_source_id, account_set_id, member_account_id),
  FOREIGN KEY (data_source_id, account_set_id) REFERENCES cala_account_sets(data_source_id, id),
  FOREIGN KEY (data_source_id, member_account_id) REFERENCES cala_accounts(data_source_id, id)
);

CREATE TABLE cala_account_set_member_account_sets (
  data_source_id UUID NOT NULL DEFAULT '00000000-0000-0000-0000-000000000000',
  account_set_id UUID NOT NULL,
  member_account_set_id UUID NOT NULL,
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  UNIQUE(data_source_id, account_set_id, member_account_set_id),
  FOREIGN KEY (data_source_id, account_set_id) REFERENCES cala_account_sets(data_source_id, id),
  FOREIGN KEY (data_source_id, member_account_set_id) REFERENCES cala_account_sets(data_source_id, id)
);

CREATE TABLE cala_tx_templates (
  data_source_id UUID NOT NULL DEFAULT '00000000-0000-0000-0000-000000000000',
  id UUID NOT NULL,
  code VARCHAR NOT NULL, 
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  UNIQUE(data_source_id, id),
  UNIQUE(data_source_id, code)
);

CREATE TABLE cala_tx_template_events (
  data_source_id UUID NOT NULL DEFAULT '00000000-0000-0000-0000-000000000000',
  id UUID NOT NULL,
  sequence INT NOT NULL,
  event_type VARCHAR NOT NULL,
  event JSONB NOT NULL,
  recorded_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  UNIQUE(data_source_id, id, sequence),
  FOREIGN KEY (data_source_id, id) REFERENCES cala_tx_templates(data_source_id, id)
);

CREATE TABLE cala_transactions (
  data_source_id UUID NOT NULL DEFAULT '00000000-0000-0000-0000-000000000000',
  id UUID NOT NULL,
  journal_id UUID NOT NULL,
  tx_template_id UUID NOT NULL,
  external_id VARCHAR DEFAULT NULL,
  correlation_id VARCHAR NOT NULL,
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  UNIQUE(data_source_id, id),
  FOREIGN KEY (data_source_id, journal_id) REFERENCES cala_journals(data_source_id, id),
  FOREIGN KEY (data_source_id, tx_template_id) REFERENCES cala_tx_templates(data_source_id, id)
);
CREATE INDEX idx_cala_transactions_data_source_id_correlation_id ON cala_transactions (data_source_id, correlation_id);
CREATE UNIQUE INDEX idx_cala_transactions_data_source_id_external_id ON cala_transactions (data_source_id, external_id) WHERE external_id IS NOT NULL;

CREATE TABLE cala_transaction_events (
  data_source_id UUID NOT NULL DEFAULT '00000000-0000-0000-0000-000000000000',
  id UUID NOT NULL,
  sequence INT NOT NULL,
  event_type VARCHAR NOT NULL,
  event JSONB NOT NULL,
  recorded_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  UNIQUE(data_source_id, id, sequence),
  FOREIGN KEY (data_source_id, id) REFERENCES cala_transactions(data_source_id, id)
);

CREATE TABLE cala_entries (
  data_source_id UUID NOT NULL DEFAULT '00000000-0000-0000-0000-000000000000',
  id UUID NOT NULL,
  journal_id UUID NOT NULL,
  account_id UUID NOT NULL,
  transaction_id UUID NOT NULL,
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  UNIQUE(data_source_id, id),
  FOREIGN KEY (data_source_id, journal_id) REFERENCES cala_journals(data_source_id, id),
  FOREIGN KEY (data_source_id, account_id) REFERENCES cala_accounts(data_source_id, id)
);
CREATE INDEX idx_cala_entries_transaction_id ON cala_entries (transaction_id);

CREATE TABLE cala_entry_events (
  data_source_id UUID NOT NULL DEFAULT '00000000-0000-0000-0000-000000000000',
  id UUID NOT NULL,
  sequence INT NOT NULL,
  event_type VARCHAR NOT NULL,
  event JSONB NOT NULL,
  recorded_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  UNIQUE(data_source_id, id, sequence),
  FOREIGN KEY (data_source_id, id) REFERENCES cala_entries(data_source_id, id)
);

CREATE TABLE cala_current_balances (
  data_source_id UUID NOT NULL DEFAULT '00000000-0000-0000-0000-000000000000',
  journal_id UUID NOT NULL,
  account_id UUID NOT NULL,
  currency VARCHAR NOT NULL,
  latest_version INT NOT NULL,
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  UNIQUE(data_source_id, journal_id, account_id, currency),
  FOREIGN KEY (data_source_id, journal_id) REFERENCES cala_journals(data_source_id, id),
  FOREIGN KEY (data_source_id, account_id) REFERENCES cala_accounts(data_source_id, id)
);

CREATE TABLE cala_balance_history (
  data_source_id UUID NOT NULL DEFAULT '00000000-0000-0000-0000-000000000000',
  journal_id UUID NOT NULL,
  account_id UUID NOT NULL,
  latest_entry_id UUID NOT NULL,
  currency VARCHAR NOT NULL,
  version INT NOT NULL,
  values JSONB NOT NULL,
  recorded_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  UNIQUE(data_source_id, journal_id, account_id, currency, version),
  FOREIGN KEY (data_source_id, journal_id, account_id, currency) REFERENCES cala_current_balances(data_source_id, journal_id, account_id, currency),
  FOREIGN KEY (data_source_id, latest_entry_id) REFERENCES cala_entries(data_source_id, id)
);
CREATE INDEX idx_cala_balance_history_recorded_at ON cala_balance_history (recorded_at);

CREATE TABLE cala_velocity_limits (
  data_source_id UUID NOT NULL DEFAULT '00000000-0000-0000-0000-000000000000',
  id UUID NOT NULL,
  name VARCHAR NOT NULL,
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  UNIQUE(data_source_id, id)
);
CREATE INDEX idx_cala_velocity_limits_name ON cala_velocity_limits (name);


CREATE TABLE cala_velocity_limit_events (
  data_source_id UUID NOT NULL DEFAULT '00000000-0000-0000-0000-000000000000',
  id UUID NOT NULL,
  sequence INT NOT NULL,
  event_type VARCHAR NOT NULL,
  event JSONB NOT NULL,
  recorded_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  UNIQUE(data_source_id, id, sequence),
  FOREIGN KEY (data_source_id, id) REFERENCES cala_velocity_limits(data_source_id, id)
);

CREATE TABLE cala_velocity_controls (
  data_source_id UUID NOT NULL DEFAULT '00000000-0000-0000-0000-000000000000',
  id UUID NOT NULL,
  name VARCHAR NOT NULL,
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  UNIQUE(data_source_id, id)
);
CREATE INDEX idx_cala_velocity_controls_name ON cala_velocity_controls (name);


CREATE TABLE cala_velocity_control_events (
  data_source_id UUID NOT NULL DEFAULT '00000000-0000-0000-0000-000000000000',
  id UUID NOT NULL,
  sequence INT NOT NULL,
  event_type VARCHAR NOT NULL,
  event JSONB NOT NULL,
  recorded_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  UNIQUE(data_source_id, id, sequence),
  FOREIGN KEY (data_source_id, id) REFERENCES cala_velocity_controls(data_source_id, id)
);

CREATE TABLE cala_outbox_events (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  sequence BIGSERIAL UNIQUE,
  payload JSONB,
  recorded_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  seen_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE FUNCTION notify_cala_outbox_events() RETURNS TRIGGER AS $$
DECLARE
  payload TEXT;
BEGIN
  payload := row_to_json(NEW);
  PERFORM pg_notify('cala_outbox_events', payload);
  RETURN NULL;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER cala_outbox_events AFTER INSERT ON cala_outbox_events
  FOR EACH ROW EXECUTE FUNCTION notify_cala_outbox_events();
