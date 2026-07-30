CREATE TABLE jobs (
  id UUID PRIMARY KEY,
  unique_per_type BOOLEAN NOT NULL,
  job_type VARCHAR NOT NULL,
  parent_job_id UUID REFERENCES jobs(id),
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE UNIQUE INDEX idx_unique_job_type ON jobs (job_type) WHERE unique_per_type = TRUE;
CREATE INDEX idx_jobs_parent_job_id ON jobs (parent_job_id);

CREATE TABLE job_events (
  id UUID NOT NULL REFERENCES jobs(id),
  sequence INT NOT NULL,
  event_type VARCHAR NOT NULL,
  event JSONB NOT NULL,
  context JSONB DEFAULT NULL,
  recorded_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  UNIQUE(id, sequence)
);

CREATE TYPE JobExecutionState AS ENUM ('pending', 'running');

CREATE TABLE job_executions (
  id UUID REFERENCES jobs(id) NOT NULL UNIQUE,
  job_type VARCHAR NOT NULL,
  queue_id VARCHAR,
  poller_instance_id UUID,
  attempt_index INT NOT NULL DEFAULT 1,
  state JobExecutionState NOT NULL DEFAULT 'pending',
  execution_state_json JSONB,
  execute_at TIMESTAMPTZ,
  alive_at TIMESTAMPTZ NOT NULL,
  created_at TIMESTAMPTZ NOT NULL
);

CREATE INDEX idx_job_executions_poller_instance
  ON job_executions(poller_instance_id)
  WHERE state = 'running';

CREATE INDEX idx_job_executions_pending_execute_at
  ON job_executions(execute_at)
  WHERE state = 'pending';

CREATE INDEX idx_job_executions_pending_job_type_execute_at
  ON job_executions(job_type, execute_at)
  WHERE state = 'pending';

CREATE INDEX idx_job_executions_running_queue_id
  ON job_executions(queue_id)
  WHERE state = 'running' AND queue_id IS NOT NULL;

-- job_executions is a small, extremely update-heavy table (state flips,
-- heartbeat bumps, per-attempt reschedules). A lowered fillfactor leaves
-- room in each page so non-indexed-column updates (execution_state_json,
-- alive_at, poller_instance_id) can go HOT instead of appending a new
-- row version elsewhere, and aggressive autovacuum settings keep dead
-- tuples near zero — cheap at this table size, and it stops the poll
-- query's cost from growing between default-schedule vacuums.
-- cost_delay = 0 removes the vacuum throttle: at this table's churn
-- rate the default 2ms delay reclaims dead tuples slower than they
-- accumulate, and the poll query's cost grows with the backlog.
ALTER TABLE job_executions SET (
  fillfactor = 70,
  autovacuum_vacuum_scale_factor = 0.01,
  autovacuum_vacuum_threshold = 50,
  autovacuum_analyze_scale_factor = 0.02,
  autovacuum_vacuum_cost_delay = 0
);

CREATE OR REPLACE FUNCTION notify_job_event() RETURNS TRIGGER AS $$
BEGIN
  IF TG_OP = 'INSERT' THEN
    PERFORM pg_notify('job_events',
      json_build_object('type', 'execution_ready', 'job_type', NEW.job_type)::text);
    RETURN NULL;
  END IF;

  IF TG_OP = 'UPDATE' THEN
    -- Only wake pollers when a row is (or becomes) eligible in the
    -- pending set. Transitions out of 'pending' (a poller taking the job
    -- sets execute_at = NULL, completion, etc.) must not notify: the
    -- poller's own UPDATE would wake every poller including itself for
    -- no reason, multiplying poll load with throughput.
    IF NEW.state = 'pending'
      AND NEW.execute_at IS DISTINCT FROM OLD.execute_at THEN
      PERFORM pg_notify('job_events',
        json_build_object('type', 'execution_ready', 'job_type', NEW.job_type)::text);
    END IF;
    RETURN NULL;
  END IF;

  IF TG_OP = 'DELETE' THEN
    PERFORM pg_notify('job_events',
      json_build_object('type', 'job_terminal', 'job_id', OLD.id::text)::text);
    IF OLD.queue_id IS NOT NULL THEN
      PERFORM pg_notify('job_events',
        json_build_object('type', 'execution_ready', 'job_type', OLD.job_type)::text);
    END IF;
    RETURN NULL;
  END IF;

  RETURN NULL;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER job_executions_notify_event_trigger
AFTER INSERT OR UPDATE OR DELETE ON job_executions
FOR EACH ROW
EXECUTE FUNCTION notify_job_event();
