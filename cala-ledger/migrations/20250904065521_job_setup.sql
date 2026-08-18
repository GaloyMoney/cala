CREATE TABLE jobs (
  id UUID PRIMARY KEY,
  unique_key VARCHAR,
  resident BOOLEAN NOT NULL DEFAULT FALSE,
  job_type VARCHAR NOT NULL,
  queue_id VARCHAR,
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
-- The two singleton flavors are fully orthogonal: `unique_key IS NOT NULL`
-- means keyed, `resident` means resident — a job is never both, and neither
-- uses a sentinel value in the other's column.
--
-- `jobs` accumulates one row per generation of a keyed job (liveness for
-- those is enforced on `job_executions`, see
-- `idx_job_executions_job_type_unique_key`), so this index is a read path
-- only: it resolves the latest generation of a `(job_type, unique_key)` for
-- `find_keyed`/`keyed_handles`.
CREATE INDEX idx_jobs_job_type_unique_key_created_at
  ON jobs (job_type, unique_key, created_at DESC)
  WHERE unique_key IS NOT NULL;

-- `ResidentJobSpawner::spawn` enforcement: absolutely unique, at most one job
-- of `job_type` EVER exists where `resident` is set. `jobs` rows are never
-- deleted, so once that job reaches a terminal state the type can never be
-- spawned again (in practice a resident job never reaches one — see
-- `resident.rs` — but the index doesn't depend on that). A dedicated boolean
-- column rather than a sentinel `unique_key` value — DB-level enforcement
-- doesn't depend on a magic string staying in sync with application code,
-- and ordinary keyed jobs (which never set this flag) are entirely outside
-- this index's partial predicate, getting LIVE-only enforcement on
-- `job_executions` instead.
CREATE UNIQUE INDEX idx_jobs_job_type_resident
  ON jobs (job_type)
  WHERE resident;

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
  unique_key VARCHAR,
  poller_instance_id UUID,
  attempt_index INT NOT NULL DEFAULT 1,
  state JobExecutionState NOT NULL DEFAULT 'pending',
  execute_at TIMESTAMPTZ,
  alive_at TIMESTAMPTZ NOT NULL,
  created_at TIMESTAMPTZ NOT NULL
);

-- Execution rows exist iff a job is pending/running (deleted on terminal, see
-- `dispatcher.rs`/`batch_dispatcher.rs`), so this index makes "at most one
-- LIVE job per (job_type, unique_key)" structural and index-exact at spawn
-- time — the enforcement point for `KeyedJobSpawner::spawn`. A key becomes
-- respawnable the instant its live row is deleted. Resident jobs never carry
-- a `unique_key` (they're enforced absolutely by `idx_jobs_job_type_resident`
-- above), so they're entirely outside this index's partial predicate.
CREATE UNIQUE INDEX idx_job_executions_job_type_unique_key
  ON job_executions (job_type, unique_key)
  WHERE unique_key IS NOT NULL;

-- The claim path's index set is measured, not assumed: see PERFORMANCE.md
-- ("Indexes: the write-path trade-off") for why it is these three and not a
-- merged or covering variant.

-- Serves `min_wait` and the stale-pending reporter. Global execute_at order,
-- which neither claim index below provides.
CREATE INDEX idx_job_executions_pending_execute_at
  ON job_executions(execute_at)
  WHERE state = 'pending';

-- CLAIM PATH, queued half. Supports both queue-walk seeks
-- (`MIN(queue_id) > cursor`) and each queue's head row
-- (`queue_id = ? ORDER BY execute_at LIMIT 1`), which is what makes claim
-- cost O(queues examined) rather than O(pending rows).
CREATE INDEX idx_job_executions_pending_queue_head
  ON job_executions(queue_id, execute_at)
  WHERE state = 'pending' AND queue_id IS NOT NULL;

-- CLAIM PATH, unqueued half. Rows without a queue_id can never be blocked by
-- a running sibling, so they are claimed in plain execute_at order, per type.
-- `job_type` MUST lead, or a registered type with no pending work scans every
-- unqueued row instead of costing one empty probe.
CREATE INDEX idx_job_executions_pending_unqueued
  ON job_executions(job_type, execute_at)
  WHERE state = 'pending' AND queue_id IS NULL;

-- Queue eligibility: does this queue already have a job running?
CREATE INDEX idx_job_executions_running_queue_id
  ON job_executions(queue_id)
  WHERE state = 'running' AND queue_id IS NOT NULL;

ALTER TABLE job_executions SET (
  fillfactor = 70,
  autovacuum_vacuum_scale_factor = 0.01,
  autovacuum_vacuum_threshold = 50,
  autovacuum_analyze_scale_factor = 0.02,
  autovacuum_vacuum_cost_delay = 0,
  log_autovacuum_min_duration = 0
);

-- Written by running jobs (attempt-recovery state, id-addressed). Deleted
-- alongside the execution row on terminal (`dispatcher.rs`'s
-- `delete_execution_in_op`), EXCEPT for keyed types that opt into
-- `KeyedJobInitializer::inherits_state`: those rows are kept so the next
-- generation of the key can seed from them, and older generations are
-- compacted away at that key's next spawn (`keyed.rs`). The table therefore
-- stays O(live jobs + inheriting keys), not O(all generations ever).
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
