-- Add migration script here

CREATE TABLE import_jobs (
  id UUID NOT NULL UNIQUE,
  name VARCHAR NOT NULL UNIQUE,
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE import_job_events (
  id UUID REFERENCES import_jobs(id) NOT NULL,
  sequence INT NOT NULL,
  event_type VARCHAR NOT NULL,
  event JSONB NOT NULL,
  recorded_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  UNIQUE(id, sequence)
);

CREATE TABLE job_executions (
  id UUID NOT NULL UNIQUE,
  job_type VARCHAR NOT NULL,
  executing_server_id VARCHAR,
  state_json JSONB,
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  reschedule_after TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
