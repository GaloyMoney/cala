-- Since obix 0.4.3 persistent-outbox notifications are emitted by the insert
-- statement itself (pg_notify with {min_sequence, max_sequence}); the per-row
-- trigger would send full-row payloads the listener can no longer decode.
DROP TRIGGER IF EXISTS cala_persistent_outbox_events_notify ON cala_persistent_outbox_events;
DROP FUNCTION IF EXISTS cala_notify_persistent_outbox_events();
