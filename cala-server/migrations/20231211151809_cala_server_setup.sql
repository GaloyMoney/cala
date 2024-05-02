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
