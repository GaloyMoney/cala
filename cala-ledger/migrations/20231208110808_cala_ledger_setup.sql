CREATE TYPE DebitOrCredit AS ENUM ('debit', 'credit');
CREATE TYPE Status AS ENUM ('active', 'locked');
CREATE TYPE Layer AS ENUM ('settled', 'pending', 'encumbered');

CREATE TABLE cala_accounts (
  connection_id UUID NOT NULL DEFAULT '00000000-0000-0000-0000-000000000000',
  id UUID NOT NULL,
  code VARCHAR NOT NULL,
  name VARCHAR NOT NULL,
  tags VARCHAR[] NOT NULL,
  external_id VARCHAR,
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  UNIQUE(connection_id, id),
  UNIQUE(connection_id, code)
);
CREATE INDEX idx_cala_accounts_name ON cala_accounts (name);
CREATE UNIQUE INDEX idx_cala_accounts_connection_id_external_id ON cala_accounts (connection_id, external_id) WHERE external_id IS NOT NULL;
CREATE INDEX idx_cala_accounts_tags ON cala_accounts USING GIN (tags);


CREATE TABLE cala_account_events (
  connection_id UUID NOT NULL DEFAULT '00000000-0000-0000-0000-000000000000',
  id UUID NOT NULL,
  sequence INT NOT NULL,
  event_type VARCHAR NOT NULL,
  event JSONB NOT NULL,
  recorded_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  UNIQUE(connection_id, id, sequence),
  FOREIGN KEY (connection_id, id) REFERENCES cala_accounts(connection_id, id)
);

CREATE TABLE cala_journals (
  connection_id UUID NOT NULL DEFAULT '00000000-0000-0000-0000-000000000000', 
  id UUID NOT NULL,
  name VARCHAR NOT NULL, 
  external_id VARCHAR,
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  UNIQUE(connection_id, id)
);
CREATE INDEX idx_cala_journals_name ON cala_journals (name);
CREATE UNIQUE INDEX idx_cala_journals_connection_id_external_id ON cala_journals (connection_id, external_id) WHERE external_id IS NOT NULL;

CREATE TABLE cala_journal_events (
  connection_id UUID NOT NULL DEFAULT '00000000-0000-0000-0000-000000000000',
  id UUID NOT NULL,
  sequence INT NOT NULL,
  event_type VARCHAR NOT NULL,
  event JSONB NOT NULL,
  recorded_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  UNIQUE(connection_id, id, sequence),
  FOREIGN KEY (connection_id, id) REFERENCES cala_journals(connection_id, id)
);

CREATE TABLE cala_outbox_events (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  sequence BIGSERIAL UNIQUE,
  payload JSONB NOT NULL,
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
