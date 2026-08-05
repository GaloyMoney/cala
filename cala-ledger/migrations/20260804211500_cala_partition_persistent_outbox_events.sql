-- Convert cala_persistent_outbox_events to the RANGE-partitioned layout
-- introduced in obix 0.6.0 (Stage 1 partitioning): primary key on `sequence`,
-- explicit partitions of width 2M tiling onto _p0, plus an always-present
-- DEFAULT backstop so inserts can never fail to route. The obix partition
-- maintainer (registered by the consuming application) keeps partitions
-- pre-created ahead of the head from here on.
--
-- The table is rewritten in place: all rows and the sequence position are
-- preserved, so consumers' sequence cursors stay valid. The unused seen_at
-- column is dropped (removed in obix 0.6.0). The copy holds ACCESS EXCLUSIVE,
-- so outbox writes BLOCK (they do not fail) for the duration of this
-- migration.

ALTER TABLE cala_persistent_outbox_events RENAME TO cala_persistent_outbox_events_old;

CREATE TABLE cala_persistent_outbox_events (
  id UUID NOT NULL DEFAULT gen_random_uuid(),
  sequence BIGINT NOT NULL DEFAULT nextval('cala_persistent_outbox_events_sequence_seq'),
  payload JSONB,
  tracing_context JSONB,
  recorded_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  PRIMARY KEY (sequence)
) PARTITION BY RANGE (sequence);

ALTER SEQUENCE cala_persistent_outbox_events_sequence_seq
  OWNED BY cala_persistent_outbox_events.sequence;

-- Cover the existing sequence range with explicit partitions (2M width, same
-- storage params the obix maintainer applies) so no copied row lands in
-- DEFAULT.
DO $$
DECLARE
  max_k BIGINT;
  k BIGINT;
BEGIN
  SELECT COALESCE(MAX(sequence), 0) / 2000000 INTO max_k
  FROM cala_persistent_outbox_events_old;
  FOR k IN 0..max_k LOOP
    EXECUTE format(
      'CREATE TABLE cala_persistent_outbox_events_p%s PARTITION OF cala_persistent_outbox_events '
      'FOR VALUES FROM (%s) TO (%s) WITH (autovacuum_vacuum_insert_scale_factor = 0.0, '
      'autovacuum_vacuum_insert_threshold = 50000, autovacuum_freeze_min_age = 0, fillfactor = 100)',
      k, k * 2000000, (k + 1) * 2000000);
  END LOOP;
END $$;

CREATE TABLE cala_persistent_outbox_events_default
  PARTITION OF cala_persistent_outbox_events DEFAULT;

-- Explicit column list: sequence is copied (no nextval), preserving every
-- row's position; seen_at is dropped.
INSERT INTO cala_persistent_outbox_events (id, sequence, payload, tracing_context, recorded_at)
  SELECT id, sequence, payload, tracing_context, recorded_at
  FROM cala_persistent_outbox_events_old;

DROP TABLE cala_persistent_outbox_events_old;
