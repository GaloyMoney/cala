CREATE TABLE jobs (
  id UUID NOT NULL UNIQUE,
  name VARCHAR NOT NULL,
  type VARCHAR NOT NULL,
  description VARCHAR,
  state_json JSONB,
  last_error VARCHAR,
  completed_at TIMESTAMPTZ,
  modified_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX idx_jobs_name ON jobs (name);

CREATE TYPE JobExecutionState AS ENUM ('pending', 'running');

CREATE TABLE job_executions (
  id UUID REFERENCES jobs(id) NOT NULL UNIQUE,
  next_attempt INT NOT NULL DEFAULT 1,
  name VARCHAR NOT NULL,
  state JobExecutionState NOT NULL DEFAULT 'pending',
  reschedule_after TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE integrations (
  id UUID PRIMARY KEY,
  name VARCHAR NOT NULL,
  data JSONB,
  modified_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
