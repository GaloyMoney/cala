CREATE TABLE jobs (
  id UUID PRIMARY KEY,
  unique_key VARCHAR,
  resident BOOLEAN NOT NULL DEFAULT FALSE,
  job_type VARCHAR NOT NULL,
  queue_id VARCHAR,
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX idx_jobs_job_type_unique_key_created_at
  ON jobs (job_type, unique_key, created_at DESC)
  WHERE unique_key IS NOT NULL;
CREATE UNIQUE INDEX idx_jobs_job_type_resident
  ON jobs (job_type)
  WHERE resident;

CREATE TABLE job_events (
  id UUID NOT NULL,
  sequence INT NOT NULL,
  event_type VARCHAR NOT NULL,
  event JSONB NOT NULL,
  context JSONB DEFAULT NULL,
  recorded_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  UNIQUE(id, sequence)
);

CREATE TYPE JobExecutionState AS ENUM ('pending', 'parked', 'running');

CREATE TABLE job_executions (
  id UUID NOT NULL UNIQUE,
  job_type VARCHAR NOT NULL,
  queue_id VARCHAR,
  unique_key VARCHAR,
  poller_instance_id UUID,
  attempt_index INT NOT NULL DEFAULT 1,
  state JobExecutionState NOT NULL DEFAULT 'pending',
  execute_at TIMESTAMPTZ,
  alive_at TIMESTAMPTZ NOT NULL,
  woken_at TIMESTAMPTZ,
  created_at TIMESTAMPTZ NOT NULL
);

CREATE UNIQUE INDEX idx_job_executions_job_type_unique_key
  ON job_executions (job_type, unique_key)
  WHERE unique_key IS NOT NULL;

CREATE INDEX idx_job_executions_pending_execute_at
  ON job_executions(job_type, execute_at, id)
  WHERE state = 'pending';

CREATE UNIQUE INDEX idx_job_executions_queue_active
  ON job_executions (queue_id)
  WHERE state IN ('pending', 'running') AND queue_id IS NOT NULL;

CREATE INDEX idx_job_executions_parked_queue_head
  ON job_executions(queue_id, execute_at, id)
  WHERE state = 'parked';

ALTER TABLE job_executions SET (
  fillfactor = 70,
  autovacuum_vacuum_scale_factor = 0.01,
  autovacuum_vacuum_threshold = 50,
  autovacuum_analyze_scale_factor = 0.02,
  autovacuum_vacuum_cost_delay = 0,
  log_autovacuum_min_duration = 0
);

CREATE TABLE job_execution_states (
  id UUID PRIMARY KEY,
  execution_state_json JSONB NOT NULL
);
ALTER TABLE job_execution_states SET (
  fillfactor = 50,
  autovacuum_vacuum_scale_factor = 0.01,
  autovacuum_vacuum_threshold = 50,
  autovacuum_analyze_scale_factor = 0.02,
  autovacuum_vacuum_cost_delay = 0
);

CREATE TABLE job_waiters (
  job_id UUID NOT NULL,
  waiter_job_id UUID NOT NULL,
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  PRIMARY KEY (job_id, waiter_job_id)
);

CREATE INDEX idx_job_waiters_waiter ON job_waiters (waiter_job_id);
