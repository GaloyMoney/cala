CREATE TABLE jobs (
  id UUID NOT NULL UNIQUE,
  name VARCHAR NOT NULL,
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX idx_jobs_name ON jobs (name);

CREATE TABLE job_events (
  id UUID REFERENCES jobs(id) NOT NULL,
  sequence INT NOT NULL,
  event_type VARCHAR NOT NULL,
  event JSONB NOT NULL,
  recorded_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  UNIQUE(id, sequence)
);

CREATE TYPE JobExecutionState AS ENUM ('pending', 'running', 'paused');

CREATE TABLE job_executions (
  id UUID REFERENCES jobs(id) NOT NULL UNIQUE,
  state JobExecutionState NOT NULL DEFAULT 'pending',
  state_json JSONB,
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  reschedule_after TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE integrations (
  id UUID PRIMARY KEY,
  name VARCHAR NOT NULL UNIQUE,
  data JSONB,
  modified_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
