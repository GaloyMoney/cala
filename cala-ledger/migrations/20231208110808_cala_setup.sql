CREATE TYPE DebitOrCredit AS ENUM ('debit', 'credit');
CREATE TYPE Status AS ENUM ('active', 'locked');
CREATE TYPE Layer AS ENUM ('settled', 'pending', 'encumbered');

CREATE TABLE cala_accounts (
  connection_id UUID NOT NULL DEFAULT '00000000-0000-0000-0000-000000000000',
  id UUID PRIMARY KEY,
  code VARCHAR NOT NULL,
  name VARCHAR NOT NULL,
  tags VARCHAR[],
  external_id VARCHAR NOT NULL,
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  UNIQUE(connection_id, id),
  UNIQUE(connection_id, code),
  UNIQUE(connection_id, name),
  UNIQUE(connection_id, external_id)
);

CREATE TABLE cala_account_events (
  id UUID REFERENCES cala_accounts(id) NOT NULL,
  sequence INT NOT NULL,
  event_type VARCHAR NOT NULL,
  event JSONB NOT NULL,
  recorded_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  UNIQUE(id, sequence)
)
