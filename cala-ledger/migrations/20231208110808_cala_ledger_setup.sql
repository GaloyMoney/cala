CREATE TYPE DebitOrCredit AS ENUM ('debit', 'credit');
CREATE TYPE Status AS ENUM ('active', 'locked');
CREATE TYPE Layer AS ENUM ('settled', 'pending', 'encumbrance');

CREATE TABLE cala_accounts (
  id UUID PRIMARY KEY,
  code VARCHAR UNIQUE NOT NULL,
  name VARCHAR NOT NULL,
  external_id VARCHAR UNIQUE,

  normal_balance_type DebitOrCredit NOT NULL, -- For quick lookup when querying balances
  eventually_consistent BOOLEAN NOT NULL, -- For balance locking
  is_account_set BOOLEAN NOT NULL, -- TRUE for account-set backing accounts
  velocity_context_values JSONB NOT NULL, -- Cached for quicker velocity enforcement
  status Status NOT NULL, -- For checking account status
  created_at TIMESTAMPTZ NOT NULL,

  UNIQUE (id, is_account_set) -- FK target: entries reference only non-set accounts
);
CREATE INDEX idx_cala_accounts_name ON cala_accounts (name);

CREATE TABLE cala_account_events (
  id UUID NOT NULL REFERENCES cala_accounts(id),
  sequence INT NOT NULL,
  event_type VARCHAR NOT NULL,
  event JSONB NOT NULL,
  context JSONB DEFAULT NULL,
  recorded_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  UNIQUE(id, sequence)
);

CREATE TABLE cala_journals (
  id UUID PRIMARY KEY,
  name VARCHAR NOT NULL,
  code VARCHAR UNIQUE DEFAULT NULL,

  created_at TIMESTAMPTZ NOT NULL
);
CREATE INDEX idx_cala_journals_name ON cala_journals (name);

CREATE TABLE cala_journal_events (
  id UUID NOT NULL REFERENCES cala_journals(id),
  sequence INT NOT NULL,
  event_type VARCHAR NOT NULL,
  event JSONB NOT NULL,
  context JSONB DEFAULT NULL,
  recorded_at TIMESTAMPTZ NOT NULL,
  UNIQUE(id, sequence)
);

CREATE TABLE cala_account_sets (
  id UUID PRIMARY KEY REFERENCES cala_accounts(id),
  journal_id UUID NOT NULL REFERENCES cala_journals(id),
  name VARCHAR NOT NULL,
  external_id VARCHAR UNIQUE,

  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX idx_cala_account_sets_name ON cala_account_sets (name);

CREATE TABLE cala_account_set_events (
  id UUID NOT NULL REFERENCES cala_account_sets(id),
  sequence INT NOT NULL,
  event_type VARCHAR NOT NULL,
  event JSONB NOT NULL,
  context JSONB DEFAULT NULL,
  recorded_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  UNIQUE(id, sequence)
);

CREATE TABLE cala_account_set_member_accounts (
  account_set_id UUID NOT NULL REFERENCES cala_account_sets(id),
  member_account_id UUID NOT NULL REFERENCES cala_accounts(id),
  -- Only **direct** account->set edges live here. Ancestor sets are
  -- resolved at read time by an upward recursive walk over
  -- cala_account_set_member_account_sets (see fetch_mappings_in_op /
  -- fetch_ec_set_mappings); there is no materialized transitive closure.
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  -- Lead the uniqueness constraint on the random member_account_id: all
  -- concurrent attaches insert into the same few hot account_set_id key
  -- ranges, which serializes them on leaf-page buffer locks. The pair is
  -- unique either way, and this column order doubles as the reverse index
  -- for account->sets resolution (fetch_mappings_in_op on the posting
  -- path), which filters member_account_id.
  UNIQUE(member_account_id, account_set_id)
);
-- Set->members lookups (balance rollup member scans) filter
-- account_set_id; kept as a plain index so the hot insert path doesn't
-- also pay unique-check page pinning on a hot leading column.
CREATE INDEX idx_cala_account_set_member_accounts_set
  ON cala_account_set_member_accounts (account_set_id);

CREATE TABLE cala_account_set_member_account_sets (
  account_set_id UUID NOT NULL REFERENCES cala_account_sets(id),
  member_account_set_id UUID NOT NULL REFERENCES cala_account_sets(id),
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  UNIQUE(account_set_id, member_account_set_id)
);
-- Reverse index for the upward `WITH RECURSIVE parents` walk, which filters and
-- joins member_account_set_id at every level; the UNIQUE above leads on account_set_id.
CREATE INDEX idx_cala_account_set_member_account_sets_member_set
  ON cala_account_set_member_account_sets (member_account_set_id, account_set_id);

-- Generation counter for the set->set edge graph, consulted by the
-- in-process set-graph cache (see account_set/graph_cache.rs). Bumped
-- ONLY by add_member_set / remove_member_set — the sole mutators of
-- cala_account_set_member_account_sets — which already hold the
-- exclusive coarse membership lock, so the bump is serialized with the
-- edge write it fences. Account-member mutations do not bump (direct
-- account->set memberships are always probed live); set creation does
-- not bump either (a fresh set is covered by the cache's unknown-id
-- fallback).
CREATE TABLE cala_account_set_graph_epoch (
  one_row BOOLEAN PRIMARY KEY DEFAULT TRUE CHECK (one_row),
  epoch BIGINT NOT NULL DEFAULT 0
);
INSERT INTO cala_account_set_graph_epoch DEFAULT VALUES;

CREATE TABLE cala_tx_templates (
  id UUID PRIMARY KEY,

  code VARCHAR UNIQUE NOT NULL,
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE cala_tx_template_events (
  id UUID NOT NULL REFERENCES cala_tx_templates(id),
  sequence INT NOT NULL,
  event_type VARCHAR NOT NULL,
  event JSONB NOT NULL,
  context JSONB DEFAULT NULL,
  recorded_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  UNIQUE(id, sequence)
);

CREATE TABLE cala_transactions (
  id UUID PRIMARY KEY,

  journal_id UUID NOT NULL,
  tx_template_id UUID NOT NULL,
  external_id VARCHAR DEFAULT NULL UNIQUE,
  correlation_id VARCHAR NOT NULL,
  effective DATE NOT NULL,
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX idx_cala_transactions_correlation_id ON cala_transactions (correlation_id);
CREATE INDEX idx_cala_transactions_effective ON cala_transactions (effective);

CREATE TABLE cala_transaction_events (
  id UUID NOT NULL REFERENCES cala_transactions(id),
  sequence INT NOT NULL,
  event_type VARCHAR NOT NULL,
  event JSONB NOT NULL,
  context JSONB DEFAULT NULL,
  recorded_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  UNIQUE(id, sequence)
);


CREATE TABLE cala_entries (
  id UUID PRIMARY KEY,
  journal_id UUID NOT NULL,
  account_id UUID NOT NULL,
  transaction_id UUID NOT NULL,
  account_is_account_set BOOLEAN NOT NULL GENERATED ALWAYS AS (FALSE) STORED, -- constant FALSE; the set-guard FK below forbids entries to account sets

  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  -- Plain existence FK, declared first so its RI check runs before the set
  -- guard: a *missing* account trips this (a generic FK error), not the set
  -- guard -- keeping "account does not exist" distinct from "account is a set".
  CONSTRAINT cala_entries_account_id_fkey
    FOREIGN KEY (account_id) REFERENCES cala_accounts(id),
  -- Set guard: only reachable when the account exists but is a set.
  CONSTRAINT cala_entries_account_not_account_set_fkey
    FOREIGN KEY (account_id, account_is_account_set) REFERENCES cala_accounts(id, is_account_set)
);
CREATE INDEX idx_cala_entries_transaction_id ON cala_entries (transaction_id);
CREATE INDEX idx_cala_entries_account_id ON cala_entries (account_id, created_at DESC, id DESC);
CREATE INDEX idx_cala_entries_journal_id_created_at ON cala_entries (journal_id, created_at DESC, id DESC);

CREATE TABLE cala_entry_events (
  id UUID NOT NULL REFERENCES cala_entries(id),
  sequence INT NOT NULL,
  event_type VARCHAR NOT NULL,
  event JSONB NOT NULL,
  context JSONB DEFAULT NULL,
  recorded_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  UNIQUE(id, sequence)
);

CREATE TABLE cala_current_balances (
  journal_id UUID NOT NULL,
  account_id UUID NOT NULL,
  currency VARCHAR NOT NULL,
  latest_version INT NOT NULL,
  latest_values JSONB NOT NULL,
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  UNIQUE(account_id, journal_id, currency)
);

-- cala_current_balances is update-heavy: every posting rewrites
-- latest_version + latest_values on an existing row. Neither column is
-- indexed (the sole index is the UNIQUE below, over immutable key columns),
-- so HOT updates are possible in principle -- but at the default
-- fillfactor = 100 the ~1KB rows (latest_values JSONB averages ~768 bytes,
-- so ~7 rows fill an 8KB page) leave no room on the page for a new version,
-- forcing a non-HOT update (new heap tuple + index entry + WAL + dead tuple)
-- on every posting. Reserving 30% of each page lets the new version stay
-- on-page and keeps the index untouched.
ALTER TABLE cala_current_balances SET (fillfactor = 70);

CREATE TABLE cala_balance_history (
  journal_id UUID NOT NULL,
  account_id UUID NOT NULL,
  latest_entry_id UUID NOT NULL,
  currency VARCHAR NOT NULL,
  version INT NOT NULL,
  values JSONB NOT NULL,
  recorded_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  UNIQUE(account_id, journal_id, currency, version),
  FOREIGN KEY (account_id, journal_id, currency) REFERENCES cala_current_balances(account_id, journal_id, currency)
);
CREATE INDEX idx_cala_balance_history_recorded_at ON cala_balance_history (recorded_at);

CREATE TABLE cala_cumulative_effective_balances (
  journal_id UUID NOT NULL,
  account_id UUID NOT NULL,
  currency VARCHAR NOT NULL,
  effective DATE NOT NULL,
  version INT NOT NULL,
  all_time_version INT NOT NULL,
  latest_entry_id UUID NOT NULL,
  values JSONB NOT NULL,
  updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  UNIQUE(journal_id, account_id, currency, effective, version)
);
CREATE INDEX idx_cala_cumeff_bal_atv_desc
ON cala_cumulative_effective_balances
  (journal_id, account_id, currency, all_time_version DESC);

CREATE TABLE cala_velocity_limits (
  id UUID PRIMARY KEY,
  name VARCHAR NOT NULL,

  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX idx_cala_velocity_limits_name ON cala_velocity_limits (name);


CREATE TABLE cala_velocity_limit_events (
  id UUID NOT NULL REFERENCES cala_velocity_limits(id),
  sequence INT NOT NULL,
  event_type VARCHAR NOT NULL,
  event JSONB NOT NULL,
  context JSONB DEFAULT NULL,
  recorded_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  UNIQUE(id, sequence)
);

CREATE TABLE cala_velocity_controls (
  id UUID PRIMARY KEY,
  name VARCHAR NOT NULL,

  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX idx_cala_velocity_controls_name ON cala_velocity_controls (name);


CREATE TABLE cala_velocity_control_events (
  id UUID NOT NULL REFERENCES cala_velocity_controls(id),
  sequence INT NOT NULL,
  event_type VARCHAR NOT NULL,
  event JSONB NOT NULL,
  context JSONB DEFAULT NULL,
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
  journal_id UUID NOT NULL,
  account_id UUID NOT NULL,
  currency VARCHAR NOT NULL,
  velocity_control_id UUID NOT NULL,
  velocity_limit_id UUID NOT NULL,
  partition_window JSONB NOT NULL,
  latest_version INT NOT NULL,
  latest_values JSONB NOT NULL,
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  UNIQUE(account_id, journal_id, currency, velocity_control_id, velocity_limit_id, partition_window)
);

-- Same shape as cala_current_balances above: postings rewrite
-- latest_version + latest_values in place, and the sole index is the UNIQUE
-- below over immutable key columns, so nothing defeats HOT except page space.
-- This table fares worse than cala_current_balances in practice (46% HOT vs
-- 65% measured under load) because partition_window JSONB makes the rows
-- wider still, so fewer fit per page and overflow comes sooner.
ALTER TABLE cala_velocity_current_balances SET (fillfactor = 70);

CREATE TABLE cala_velocity_balance_history (
  journal_id UUID NOT NULL,
  account_id UUID NOT NULL,
  currency VARCHAR NOT NULL,
  velocity_control_id UUID NOT NULL,
  velocity_limit_id UUID NOT NULL,
  partition_window JSONB NOT NULL,
  latest_entry_id UUID NOT NULL,
  version INT NOT NULL,
  values JSONB NOT NULL,
  recorded_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  UNIQUE(account_id, journal_id, currency, velocity_control_id, velocity_limit_id, partition_window, version),
  FOREIGN KEY (account_id, journal_id, currency, velocity_control_id, velocity_limit_id, partition_window) REFERENCES cala_velocity_current_balances(account_id, journal_id, currency, velocity_control_id, velocity_limit_id, partition_window)
);
