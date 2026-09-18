CREATE TABLE cala_persistent_outbox_events (
  id UUID NOT NULL DEFAULT gen_random_uuid(),
  sequence BIGSERIAL,
  payload JSONB,
  tracing_context JSONB,
  recorded_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  commit_xid BIGINT NOT NULL DEFAULT pg_current_xact_id()::text::bigint,
  PRIMARY KEY (sequence)
) PARTITION BY RANGE (sequence);

CREATE INDEX cala_idx_persistent_outbox_events_commit_xid
  ON cala_persistent_outbox_events (commit_xid);

CREATE TABLE cala_persistent_outbox_events_p0 PARTITION OF cala_persistent_outbox_events
  FOR VALUES FROM (0) TO (2000000)
  WITH (autovacuum_vacuum_insert_scale_factor = 0.0,
        autovacuum_vacuum_insert_threshold = 50000,
        autovacuum_freeze_min_age = 0,
        fillfactor = 100);

CREATE TABLE cala_persistent_outbox_events_default
  PARTITION OF cala_persistent_outbox_events DEFAULT;

CREATE TABLE cala_persistent_outbox_commit_checkpoints (
  sequence    BIGINT PRIMARY KEY,
  commit_seq  BIGINT NOT NULL UNIQUE,
  open_groups JSONB  NOT NULL,
  recorded_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE cala_ephemeral_outbox_events (
  event_type VARCHAR NOT NULL UNIQUE,
  payload JSONB NOT NULL,
  tracing_context JSONB,
  recorded_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

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

CREATE TYPE InboxEventStatus AS ENUM ('pending', 'processing', 'completed', 'failed');

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
CREATE INDEX cala_idx_subscriptions_checkpoint ON cala_subscriptions (checkpoint);
CREATE INDEX cala_idx_subscriptions_wake_keys ON cala_subscriptions USING GIN (wake_keys);
