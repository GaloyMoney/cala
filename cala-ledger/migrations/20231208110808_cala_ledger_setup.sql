CREATE TYPE DebitOrCredit AS ENUM ('debit', 'credit');
CREATE TYPE Status AS ENUM ('active', 'locked');
CREATE TYPE Layer AS ENUM ('settled', 'pending', 'encumbrance');

CREATE TABLE cala_accounts (
  id UUID PRIMARY KEY,
  code VARCHAR UNIQUE NOT NULL,
  name VARCHAR NOT NULL,
  external_id VARCHAR UNIQUE,
  data_source_id UUID NOT NULL,
  normal_balance_type DebitOrCredit NOT NULL, -- For quick lookup when querying balances
  eventually_consistent BOOLEAN NOT NULL, -- For balance locking
  latest_values JSONB NOT NULL, -- Cached for quicker velocity enforcement
  created_at TIMESTAMPTZ NOT NULL
);
CREATE INDEX idx_cala_accounts_name ON cala_accounts (name);

CREATE TABLE cala_account_events (
  id UUID NOT NULL REFERENCES cala_accounts(id),
  sequence INT NOT NULL,
  event_type VARCHAR NOT NULL,
  event JSONB NOT NULL,
  recorded_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  UNIQUE(id, sequence)
);

CREATE TABLE cala_journals (
  id UUID PRIMARY KEY,
  name VARCHAR NOT NULL,
  data_source_id UUID NOT NULL,
  created_at TIMESTAMPTZ NOT NULL
);
CREATE INDEX idx_cala_journals_name ON cala_journals (name);

CREATE TABLE cala_journal_events (
  id UUID NOT NULL REFERENCES cala_journals(id),
  sequence INT NOT NULL,
  event_type VARCHAR NOT NULL,
  event JSONB NOT NULL,
  recorded_at TIMESTAMPTZ NOT NULL,
  UNIQUE(id, sequence)
);

CREATE TABLE cala_account_sets (
  id UUID PRIMARY KEY REFERENCES cala_accounts(id),
  journal_id UUID NOT NULL REFERENCES cala_journals(id),
  name VARCHAR NOT NULL,
  data_source_id UUID NOT NULL,
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX idx_cala_account_sets_name ON cala_account_sets (name);


CREATE TABLE cala_account_set_events (
  id UUID NOT NULL REFERENCES cala_account_sets(id),
  sequence INT NOT NULL,
  event_type VARCHAR NOT NULL,
  event JSONB NOT NULL,
  recorded_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  UNIQUE(id, sequence)
);

CREATE TABLE cala_account_set_member_accounts (
  account_set_id UUID NOT NULL REFERENCES cala_account_sets(id),
  member_account_id UUID NOT NULL REFERENCES cala_accounts(id),
  transitive BOOLEAN NOT NULL DEFAULT FALSE,
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  UNIQUE(account_set_id, member_account_id)
);

CREATE TABLE cala_account_set_member_account_sets (
  account_set_id UUID NOT NULL REFERENCES cala_account_sets(id),
  member_account_set_id UUID NOT NULL REFERENCES cala_account_sets(id),
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  UNIQUE(account_set_id, member_account_set_id)
);

CREATE TABLE cala_tx_templates (
  id UUID PRIMARY KEY,
  data_source_id UUID NOT NULL,
  code VARCHAR NOT NULL, 
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE cala_tx_template_events (
  id UUID NOT NULL,
  sequence INT NOT NULL,
  event_type VARCHAR NOT NULL,
  event JSONB NOT NULL,
  recorded_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  UNIQUE(id, sequence),
  FOREIGN KEY (id) REFERENCES cala_tx_templates(id)
);

CREATE TABLE cala_transactions (
  id UUID PRIMARY KEY,
  data_source_id UUID NOT NULL,
  journal_id UUID NOT NULL,
  tx_template_id UUID NOT NULL,
  external_id VARCHAR DEFAULT NULL,
  correlation_id VARCHAR NOT NULL,
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  FOREIGN KEY (journal_id) REFERENCES cala_journals(id),
  FOREIGN KEY (tx_template_id) REFERENCES cala_tx_templates(id)
);

CREATE TABLE cala_transaction_events (
  id UUID NOT NULL REFERENCES cala_transactions(id),
  sequence INT NOT NULL,
  event_type VARCHAR NOT NULL,
  event JSONB NOT NULL,
  recorded_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  UNIQUE(id, sequence)
);

CREATE TABLE cala_entries (
  id UUID PRIMARY KEY,
  journal_id UUID NOT NULL REFERENCES cala_journals(id),
  account_id UUID NOT NULL REFERENCES cala_accounts(id),
  transaction_id UUID NOT NULL,
  data_source_id UUID NOT NULL,
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX idx_cala_entries_transaction_id ON cala_entries (transaction_id);

CREATE TABLE cala_entry_events (
  id UUID NOT NULL REFERENCES cala_entries(id),
  sequence INT NOT NULL,
  event_type VARCHAR NOT NULL,
  event JSONB NOT NULL,
  recorded_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  UNIQUE(id, sequence)
);

CREATE TABLE cala_current_balances (
  journal_id UUID NOT NULL REFERENCES cala_journals(id),
  account_id UUID NOT NULL REFERENCES cala_accounts(id),
  currency VARCHAR NOT NULL,
  latest_version INT NOT NULL,
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  UNIQUE(journal_id, account_id, currency)
);

CREATE TABLE cala_balance_history (
  journal_id UUID NOT NULL,
  account_id UUID NOT NULL,
  latest_entry_id UUID NOT NULL REFERENCES cala_entries(id),
  currency VARCHAR NOT NULL,
  version INT NOT NULL,
  values JSONB NOT NULL,
  recorded_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  UNIQUE(journal_id, account_id, currency, version),
  FOREIGN KEY (journal_id, account_id, currency) REFERENCES cala_current_balances(journal_id, account_id, currency)
);
CREATE INDEX idx_cala_balance_history_recorded_at ON cala_balance_history (recorded_at);

CREATE TABLE cala_velocity_limits (
  id UUID PRIMARY KEY,
  name VARCHAR NOT NULL,
  data_source_id UUID NOT NULL,
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX idx_cala_velocity_limits_name ON cala_velocity_limits (name);


CREATE TABLE cala_velocity_limit_events (
  id UUID NOT NULL REFERENCES cala_velocity_limits(id),
  sequence INT NOT NULL,
  event_type VARCHAR NOT NULL,
  event JSONB NOT NULL,
  recorded_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  UNIQUE(id, sequence)
);

CREATE TABLE cala_velocity_controls (
  id UUID PRIMARY KEY,
  name VARCHAR NOT NULL,
  data_source_id UUID NOT NULL,
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX idx_cala_velocity_controls_name ON cala_velocity_controls (name);


CREATE TABLE cala_velocity_control_events (
  id UUID NOT NULL REFERENCES cala_velocity_controls(id),
  sequence INT NOT NULL,
  event_type VARCHAR NOT NULL,
  event JSONB NOT NULL,
  recorded_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  UNIQUE(id, sequence)
);

CREATE TABLE cala_velocity_control_limits (
  velocity_control_id UUID NOT NULL REFERENCES cala_velocity_controls(id),
  velocity_limit_id UUID NOT NULL REFERENCES cala_velocity_limits(id),
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  UNIQUE(velocity_control_id, velocity_limit_id)
);

CREATE TABLE cala_velocity_account_controls (
  account_id UUID NOT NULL REFERENCES cala_accounts(id),
  velocity_control_id UUID NOT NULL REFERENCES cala_velocity_controls(id),
  values JSONB NOT NULL,
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  UNIQUE(account_id, velocity_control_id)
);

CREATE TABLE cala_velocity_current_balances (
  journal_id UUID NOT NULL REFERENCES cala_journals(id),
  account_id UUID NOT NULL REFERENCES cala_accounts(id),
  currency VARCHAR NOT NULL,
  velocity_control_id UUID NOT NULL REFERENCES cala_velocity_controls(id),
  velocity_limit_id UUID NOT NULL REFERENCES cala_velocity_limits(id),
  partition_window JSONB NOT NULL,
  latest_version INT NOT NULL,
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  UNIQUE(partition_window, currency, journal_id, account_id, velocity_limit_id, velocity_control_id)
);

CREATE TABLE cala_velocity_balance_history (
  journal_id UUID NOT NULL,
  account_id UUID NOT NULL,
  currency VARCHAR NOT NULL,
  velocity_control_id UUID NOT NULL,
  velocity_limit_id UUID NOT NULL,
  partition_window JSONB NOT NULL,
  latest_entry_id UUID NOT NULL REFERENCES cala_entries(id),
  version INT NOT NULL,
  values JSONB NOT NULL,
  recorded_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  UNIQUE(partition_window, currency, journal_id, account_id, velocity_limit_id, velocity_control_id, version),
  FOREIGN KEY (partition_window, currency, journal_id, account_id, velocity_limit_id, velocity_control_id) REFERENCES cala_velocity_current_balances(partition_window, currency, journal_id, account_id, velocity_limit_id, velocity_control_id)
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
