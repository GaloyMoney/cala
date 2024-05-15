CREATE TYPE DebitOrCredit AS ENUM ('debit', 'credit');
CREATE TYPE Status AS ENUM ('active', 'locked');
CREATE TYPE Layer AS ENUM ('settled', 'pending', 'encumbered');

CREATE TABLE cala_accounts (
  data_source_id UUID NOT NULL DEFAULT '00000000-0000-0000-0000-000000000000',
  id UUID NOT NULL,
  code VARCHAR NOT NULL,
  name VARCHAR NOT NULL,
  tags VARCHAR[] NOT NULL,
  external_id VARCHAR,
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  UNIQUE(data_source_id, id),
  UNIQUE(data_source_id, code)
);
CREATE INDEX idx_cala_accounts_name ON cala_accounts (name);
CREATE UNIQUE INDEX idx_cala_accounts_data_source_id_external_id ON cala_accounts (data_source_id, external_id) WHERE external_id IS NOT NULL;
CREATE INDEX idx_cala_accounts_tags ON cala_accounts USING GIN (tags);


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
  external_id VARCHAR,
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  UNIQUE(data_source_id, id)
);
CREATE INDEX idx_cala_journals_name ON cala_journals (name);
CREATE UNIQUE INDEX idx_cala_journals_data_source_id_external_id ON cala_journals (data_source_id, external_id) WHERE external_id IS NOT NULL;

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
  external_id VARCHAR DEFAULT NULL,
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  UNIQUE(data_source_id, id),
  FOREIGN KEY (data_source_id, journal_id) REFERENCES cala_journals(data_source_id, id)
);
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
  FOREIGN KEY (data_source_id, account_id) REFERENCES cala_accounts(data_source_id, id),
  FOREIGN KEY (data_source_id, transaction_id) REFERENCES cala_transactions(data_source_id, id)
);

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

CREATE TABLE cala_balances (
  data_source_id UUID NOT NULL DEFAULT '00000000-0000-0000-0000-000000000000',
  journal_id UUID NOT NULL,
  account_id UUID NOT NULL,
  currency VARCHAR NOT NULL,
  latest_version INT NOT NULL,
  current_values JSONB NOT NULL,
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  UNIQUE(data_source_id, journal_id, account_id, currency),
  FOREIGN KEY (data_source_id, journal_id) REFERENCES cala_journals(data_source_id, id),
  FOREIGN KEY (data_source_id, account_id) REFERENCES cala_accounts(data_source_id, id)
);

CREATE TABLE cala_balance_history (
  data_source_id UUID NOT NULL DEFAULT '00000000-0000-0000-0000-000000000000',
  journal_id UUID NOT NULL,
  account_id UUID NOT NULL,
  currency VARCHAR NOT NULL,
  version INT NOT NULL,
  values JSONB NOT NULL,
  recorded_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  UNIQUE(data_source_id, journal_id, account_id, currency, version),
  FOREIGN KEY (data_source_id, journal_id, account_id, currency) REFERENCES cala_balances(data_source_id, journal_id, account_id, currency)
);

CREATE TABLE cala_outbox_events (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  sequence BIGSERIAL UNIQUE,
  payload JSONB,
  recorded_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
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
