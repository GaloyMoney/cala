CREATE TABLE cala_persistent_outbox_events (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  sequence BIGSERIAL UNIQUE,
  payload JSONB,
  tracing_context JSONB,
  recorded_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  seen_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE FUNCTION cala_notify_persistent_outbox_events() RETURNS TRIGGER AS $$
DECLARE
  payload TEXT;
  payload_size INTEGER;
BEGIN
  payload := row_to_json(NEW);
  payload_size := octet_length(payload);
  IF payload_size > 8000 THEN
    payload := json_build_object(
      'id', NEW.id,
      'sequence', NEW.sequence,
      'payload', NULL,
      'payload_omitted', true,
      'tracing_context', NEW.tracing_context,
      'recorded_at', NEW.recorded_at,
      'seen_at', NEW.seen_at
    )::TEXT;
  END IF;
  PERFORM pg_notify('cala_persistent_outbox_events', payload);
  RETURN NULL;
END;
$$ LANGUAGE plpgsql;
CREATE TRIGGER cala_persistent_outbox_events AFTER INSERT ON cala_persistent_outbox_events
  FOR EACH ROW EXECUTE FUNCTION cala_notify_persistent_outbox_events();
CREATE TABLE cala_ephemeral_outbox_events (
  event_type VARCHAR NOT NULL UNIQUE,
  payload JSONB NOT NULL,
  tracing_context JSONB,
  recorded_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE FUNCTION cala_notify_ephemeral_outbox_events() RETURNS TRIGGER AS $$
DECLARE
  payload TEXT;
  payload_size INTEGER;
BEGIN
  payload := row_to_json(NEW);
  payload_size := octet_length(payload);
  IF payload_size > 8000 THEN
    payload := json_build_object(
      'event_type', NEW.event_type,
      'payload', NULL,
      'payload_omitted', true,
      'tracing_context', NEW.tracing_context,
      'recorded_at', NEW.recorded_at
    )::TEXT;
  END IF;
  PERFORM pg_notify('cala_ephemeral_outbox_events', payload);
  RETURN NULL;
END;
$$ LANGUAGE plpgsql;
CREATE TRIGGER cala_ephemeral_outbox_events_notify
  AFTER INSERT OR UPDATE ON cala_ephemeral_outbox_events
  FOR EACH ROW EXECUTE FUNCTION cala_notify_ephemeral_outbox_events();

