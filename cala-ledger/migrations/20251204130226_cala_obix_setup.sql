-- Persistent outbox events: a RANGE-partitioned table.
--
-- Partitioned by `sequence` with a DEFAULT catch-all partition, so an insert
-- can never fail to route. The primary key is `sequence` (a partitioned
-- table's PK must include the partition key); `id` is a plain column.
-- Partitions are pre-created ahead of the head by the obix partition
-- maintainer job (registered by the application).
CREATE TABLE cala_persistent_outbox_events (
  id UUID NOT NULL DEFAULT gen_random_uuid(),
  sequence BIGSERIAL,
  payload JSONB,
  tracing_context JSONB,
  recorded_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  PRIMARY KEY (sequence)
) PARTITION BY RANGE (sequence);

-- Initial partition. Its range MUST equal DEFAULT_PARTITION_WIDTH (a fixed
-- constant) so maintainer-created partitions tile onto it without overlapping.
-- Storage params are set per-partition (not inherited via PARTITION OF).
CREATE TABLE cala_persistent_outbox_events_p0 PARTITION OF cala_persistent_outbox_events
  FOR VALUES FROM (0) TO (2000000)
  WITH (autovacuum_vacuum_insert_scale_factor = 0.0,
        autovacuum_vacuum_insert_threshold = 50000,
        autovacuum_freeze_min_age = 0,
        fillfactor = 100);

-- Always-empty backstop so INSERT routing never fails if the maintainer falls
-- behind. Rows landing here are still read normally; draining them is a layout
-- repair (`Partitions::recover_default`), not a correctness failure.
CREATE TABLE cala_persistent_outbox_events_default
  PARTITION OF cala_persistent_outbox_events DEFAULT;

-- Ephemeral outbox events
CREATE TABLE cala_ephemeral_outbox_events (
  event_type VARCHAR NOT NULL UNIQUE,
  payload JSONB NOT NULL,
  tracing_context JSONB,
  recorded_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- SECURITY: the ephemeral notification is a hint, not a transport.
--
-- PostgreSQL performs no authorization on LISTEN/NOTIFY channels: any role
-- able to connect to the database could LISTEN and harvest payloads with no
-- table grant, or pg_notify a forged event consumers would accept. The
-- notification therefore carries only {event_type, recorded_at}; listeners
-- always fetch the payload from the table with their own credentials
-- (recorded_at lets them skip the fetch when their cache is already current).
CREATE FUNCTION cala_notify_ephemeral_outbox_events() RETURNS TRIGGER AS $$
BEGIN
  PERFORM pg_notify(
    'cala_ephemeral_outbox_events',
    json_build_object('event_type', NEW.event_type, 'recorded_at', NEW.recorded_at)::TEXT
  );
  RETURN NULL;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER cala_ephemeral_outbox_events_notify
  AFTER INSERT OR UPDATE ON cala_ephemeral_outbox_events
  FOR EACH ROW EXECUTE FUNCTION cala_notify_ephemeral_outbox_events();

-- Inbox events
DO $$ BEGIN
    CREATE TYPE InboxEventStatus AS ENUM ('pending', 'processing', 'completed', 'failed');
EXCEPTION
    WHEN duplicate_object THEN null;
END $$;

CREATE TABLE cala_inbox_events (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  idempotency_key VARCHAR UNIQUE,
  payload JSONB NOT NULL,
  status InboxEventStatus NOT NULL DEFAULT 'pending',
  error VARCHAR,
  recorded_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  processed_at TIMESTAMPTZ
);

CREATE INDEX cala_idx_inbox_events_status ON cala_inbox_events(status)
  WHERE status IN ('pending', 'processing', 'failed');

-- Keyed-subscriber subscriptions: one row per (subscriber_type, key)
-- identity. Row presence IS the subscription: absence means cancelled.
-- Identity and terms only (key, wake keys, instance config, birth
-- frontier); execution and progress live in the job crate's tables.
--
-- `wake_keys` are a liveness signal, not a delivery filter: they decide
-- whom to wake when a member has passivated, matched by set overlap.
-- Never empty (rejected at subscribe time).
--
-- `checkpoint` mirrors the member's durable cursor, written in the same
-- transaction as the job's own checkpoint, so the waker can find members
-- drifting out of the in-memory event cache without joining across the
-- schema boundary. It is a lower bound: a stale value costs at worst a
-- spurious, idempotent wake.
CREATE TABLE cala_subscriptions (
  subscriber_type  VARCHAR NOT NULL,
  key              VARCHAR NOT NULL,
  wake_keys        VARCHAR[] NOT NULL CHECK (cardinality(wake_keys) > 0),
  instance_config  JSONB NOT NULL,
  start_after      BIGINT NOT NULL,
  checkpoint       BIGINT NOT NULL,
  created_at       TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  PRIMARY KEY (subscriber_type, key)
);

-- Backs the waker's catch-up scan: the members nearest the eviction cliff
-- are woken first.
CREATE INDEX cala_idx_subscriptions_checkpoint ON cala_subscriptions (checkpoint);

-- Backs the waker's flush-time lookup on `wake_keys && $2::varchar[]`.
CREATE INDEX cala_idx_subscriptions_wake_keys ON cala_subscriptions USING GIN (wake_keys);
