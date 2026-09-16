-- Persistent outbox events: a RANGE-partitioned table.
--
-- Partitioned by `sequence` with a DEFAULT catch-all partition, so an insert
-- can never fail to route. The primary key is `sequence` (a partitioned
-- table's PK must include the partition key); `id` is a plain column.
-- Partitions are pre-created ahead of the head by the maintainer job in
-- `src/out/partition`.
-- `commit_xid` holds the writer's top-level transaction id. Rows sharing one
-- value committed together. `pg_current_xact_id()` returns `xid8`, which is
-- 64-bit and does not wrap; there is no direct cast to bigint, hence the text
-- hop.
CREATE TABLE cala_persistent_outbox_events (
  id UUID NOT NULL DEFAULT gen_random_uuid(),
  sequence BIGSERIAL,
  payload JSONB,
  tracing_context JSONB,
  recorded_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  commit_xid BIGINT NOT NULL DEFAULT pg_current_xact_id()::text::bigint,
  PRIMARY KEY (sequence)
) PARTITION BY RANGE (sequence);

-- Backs the sequencer's per-group lookup: every row of one `commit_xid`.
-- Created on the parent; Postgres cascades it to existing and future
-- partitions.
CREATE INDEX cala_idx_persistent_outbox_events_commit_xid
  ON cala_persistent_outbox_events (commit_xid);

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

-- Commit-ordered delivery lane: the materialised commit order, appended by
-- the sequencer (`src/out/persistent/sequencer.rs`). A group is appended
-- whole when its lowest member is reached in insert order, members by
-- `sequence` within it, and the resulting `commit_seq` is dense.
--
-- Positions only; payloads are reached by joining on `sequence`. Retention
-- must therefore drop log partitions before or with the event partitions
-- they reference, never after.
CREATE TABLE cala_persistent_outbox_commit_log (
  commit_seq BIGINT NOT NULL,
  sequence   BIGINT NOT NULL,
  group_last BOOLEAN NOT NULL,
  PRIMARY KEY (commit_seq)
) PARTITION BY RANGE (commit_seq);

-- Range equal to DEFAULT_PARTITION_WIDTH, as for the events table, so
-- maintainer-created partitions tile onto it without overlapping.
CREATE TABLE cala_persistent_outbox_commit_log_p0 PARTITION OF cala_persistent_outbox_commit_log
  FOR VALUES FROM (0) TO (2000000)
  WITH (autovacuum_vacuum_insert_scale_factor = 0.0,
        autovacuum_vacuum_insert_threshold = 50000,
        autovacuum_freeze_min_age = 0,
        fillfactor = 100);

CREATE TABLE cala_persistent_outbox_commit_log_default
  PARTITION OF cala_persistent_outbox_commit_log DEFAULT;

-- Backs the sequencer's restart read: the sequences above
-- `logged_through_sequence` that are already logged. Not UNIQUE — a
-- partitioned table's UNIQUE must include the partition key, which here is
-- `commit_seq`.
CREATE INDEX cala_idx_persistent_outbox_commit_log_sequence
  ON cala_persistent_outbox_commit_log (sequence);

-- Sequencer state. The two watermarks count different things:
-- `last_commit_seq` is a `commit_seq` (a position in the commit log),
-- `logged_through_sequence` is an insert `sequence` (a position in
-- `persistent_outbox_events`). Appending a group of three advances the
-- first by three and sets the second to that group's lowest member, so
-- neither tracks the other.
--
-- Every payload-bearing event at or below `logged_through_sequence` is in
-- the log; events above it may be too, when a group straddles it. An append
-- locks this row and is conditional on `logged_through_sequence` being below
-- the appending sequence, which is what makes concurrent sequencers append
-- each group once.
--
-- `singleton` admits one row and no other: the CHECK allows only TRUE and
-- the primary key allows only one of it.
CREATE TABLE cala_persistent_outbox_commit_log_state (
  singleton               BOOLEAN PRIMARY KEY DEFAULT TRUE CHECK (singleton),
  last_commit_seq         BIGINT NOT NULL,
  logged_through_sequence BIGINT NOT NULL
);
INSERT INTO cala_persistent_outbox_commit_log_state (last_commit_seq, logged_through_sequence)
VALUES (0, 0) ON CONFLICT (singleton) DO NOTHING;

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
-- identity.
--
-- Row presence IS the subscription: absence means cancelled. This table
-- holds identity and terms only (key, wake keys, instance config, birth
-- frontier); execution and progress (liveness, generations, attempts,
-- watermark) live entirely in the job crate's own tables, addressed by
-- (subscriber_type, key) through job's keyed-job machinery. Readers here
-- must never join against job-crate tables to decide wakes (schema
-- boundary).
--
-- `wake_keys` are a liveness signal, NOT a delivery filter: a live member
-- reads the whole stream from its own cursor and decides per event in its
-- own handler. These keys only decide whom to *wake* when a member has
-- passivated. Matching is set-overlap on both sides — an event classifies
-- to a set of wake keys, a subscription declares the set it watches, and an
-- intersection respawns it. Never empty (rejected at subscribe time): an
-- empty set overlaps nothing, so such a row could never be woken again once
-- it passivated.
--
-- A set rather than a scalar because a subscription's identity is its
-- `key`, so watching several partitions of the stream cannot be expressed
-- as extra rows.
-- `checkpoint` mirrors the member's durable cursor, written in the same
-- transaction as the job's own checkpoint. The authoritative cursor still
-- lives in the job crate's execution state; this is a copy obix owns so the
-- waker can ask "who has fallen far enough behind the in-memory event cache
-- that waking them now would save a paged cold read from disk" without
-- joining across the schema boundary into job-crate tables.
--
-- It is a lower bound, never an over-estimate: a member that dies without
-- passivating leaves it behind its true position, which costs at worst a
-- spurious wake (idempotent, resolves to the live holder or an empty run).
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

-- Backs the waker's catch-up scan: `WHERE checkpoint < $1 ORDER BY
-- checkpoint ASC LIMIT $2` across every subscriber type, so the members
-- nearest the eviction cliff are the ones woken first and the per-pass
-- limit bounds the wake rate.
CREATE INDEX cala_idx_subscriptions_checkpoint ON cala_subscriptions (checkpoint);

-- Backs the waker's flush-time lookup: `WHERE subscriber_type = $1 AND
-- wake_keys && $2::varchar[]`. The cast is load-bearing — Postgres has no
-- implicit varchar[]/text[] cast for `&&`, and sqlx infers text[] for an
-- untyped array parameter.
CREATE INDEX cala_idx_subscriptions_wake_keys ON cala_subscriptions USING GIN (wake_keys);
