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

CREATE INDEX idx_job_executions_running_alive_at
  ON job_executions(alive_at)
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

CREATE OR REPLACE FUNCTION notify_job_event() RETURNS TRIGGER AS $$
BEGIN
  IF TG_OP = 'INSERT' THEN
    PERFORM pg_notify('job_events',
      json_build_object('type', 'execution_ready', 'job_type', NEW.job_type)::text);
    RETURN NULL;
  END IF;

  IF TG_OP = 'UPDATE' THEN
    IF NEW.execute_at IS DISTINCT FROM OLD.execute_at THEN
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
