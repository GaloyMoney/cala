-- Which clock a pending row's `execute_at` deadline is measured on.
--
-- 'app' (the default, and the only behavior before this column): the
-- application clock the job service was configured with — realtime in
-- production, possibly simulated/manual in sandboxes and tests. Retries and
-- schedules are written as `execute_at = app_now + delay`, so on a clock
-- that has stopped advancing they can never come due.
--
-- 'wall': the wall clock. A job type whose failures are operational rather
-- than business-shaped (an outbox relay mirror blocked by an upstream 429,
-- say) declares `RetrySettings { retry_clock: Wall, .. }`; its failure
-- retries and congestion reschedules are then written as
-- `execute_at = wall_now + delay`, so they fire in real time even when the
-- application clock is frozen.
CREATE TYPE JobRetryClock AS ENUM ('app', 'wall');

ALTER TABLE job_executions
  ADD COLUMN retry_clock JobRetryClock NOT NULL DEFAULT 'app';
