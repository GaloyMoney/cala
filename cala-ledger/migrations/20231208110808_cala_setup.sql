CREATE TYPE DebitOrCredit AS ENUM ('debit', 'credit');
CREATE TYPE Status AS ENUM ('active', 'locked');
CREATE TYPE Layer AS ENUM ('settled', 'pending', 'encumbered');

CREATE TABLE cala_accounts (
  connection_id UUID NOT NULL,
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
