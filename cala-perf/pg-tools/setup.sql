-- =====================================================
-- PostgreSQL Performance Profiling Setup Script
-- Run this before your benchmark to enable comprehensive monitoring
-- =====================================================

-- 1. Enable pg_stat_statements extension (most important for query profiling)
CREATE EXTENSION IF NOT EXISTS pg_stat_statements;

-- 2. Configure query logging and statistics
ALTER SYSTEM SET shared_preload_libraries = 'pg_stat_statements';  -- Requires restart if not already loaded

-- Log all queries (temporarily for benchmarking - this can impact performance)
ALTER SYSTEM SET log_min_duration_statement = 0;  -- logs ALL queries; use -1 to disable after benchmark
ALTER SYSTEM SET log_statement = 'all';
ALTER SYSTEM SET log_duration = on;
ALTER SYSTEM SET log_lock_waits = on;
ALTER SYSTEM SET log_temp_files = 0;

-- Better log formatting
ALTER SYSTEM SET log_line_prefix = '%t [%p]: [%l-1] db=%d,user=%u,app=%a,client=%h ';

-- Track more statement statistics
ALTER SYSTEM SET pg_stat_statements.track = 'all';
ALTER SYSTEM SET pg_stat_statements.track_utility = on;
ALTER SYSTEM SET pg_stat_statements.track_planning = on;
ALTER SYSTEM SET track_io_timing = on;
ALTER SYSTEM SET track_functions = 'all';

-- 3. Apply configuration changes
SELECT pg_reload_conf();

-- 4. Reset existing statistics for clean benchmark
SELECT pg_stat_reset();
SELECT pg_stat_statements_reset();
SELECT pg_stat_reset_shared('bgwriter');

-- 5. Create helper views for analysis

-- View for currently running queries
CREATE OR REPLACE VIEW current_queries AS
SELECT 
    pid,
    now() - pg_stat_activity.query_start AS duration,
    wait_event_type,
    wait_event,
    state,
    query
FROM pg_stat_activity
WHERE state != 'idle' 
    AND query NOT ILIKE '%pg_stat_activity%'
ORDER BY duration DESC;

-- View for blocking queries
CREATE OR REPLACE VIEW blocking_queries AS
SELECT 
    blocked_locks.pid AS blocked_pid,
    blocked_activity.usename AS blocked_user,
    blocking_locks.pid AS blocking_pid,
    blocking_activity.usename AS blocking_user,
    blocked_activity.query AS blocked_statement,
    blocking_activity.query AS blocking_statement,
    now() - blocked_activity.query_start AS blocked_duration,
    now() - blocking_activity.query_start AS blocking_duration
FROM pg_catalog.pg_locks blocked_locks
JOIN pg_catalog.pg_stat_activity blocked_activity ON blocked_activity.pid = blocked_locks.pid
JOIN pg_catalog.pg_locks blocking_locks 
    ON blocking_locks.locktype = blocked_locks.locktype
    AND blocking_locks.database IS NOT DISTINCT FROM blocked_locks.database
    AND blocking_locks.relation IS NOT DISTINCT FROM blocked_locks.relation
    AND blocking_locks.page IS NOT DISTINCT FROM blocked_locks.page
    AND blocking_locks.tuple IS NOT DISTINCT FROM blocked_locks.tuple
    AND blocking_locks.virtualxid IS NOT DISTINCT FROM blocked_locks.virtualxid
    AND blocking_locks.transactionid IS NOT DISTINCT FROM blocked_locks.transactionid
    AND blocking_locks.classid IS NOT DISTINCT FROM blocked_locks.classid
    AND blocking_locks.objid IS NOT DISTINCT FROM blocked_locks.objid
    AND blocking_locks.objsubid IS NOT DISTINCT FROM blocked_locks.objsubid
    AND blocking_locks.pid != blocked_locks.pid
JOIN pg_catalog.pg_stat_activity blocking_activity ON blocking_activity.pid = blocking_locks.pid
WHERE NOT blocked_locks.granted;

-- 6. Warm up the database (optional but recommended)
-- This ensures caches are warm for consistent benchmark results

-- Analyze all tables to update statistics
ANALYZE;

-- 7. Display current configuration
SELECT name, setting, unit, short_desc
FROM pg_settings
WHERE name IN (
    'log_min_duration_statement',
    'log_statement',
    'log_duration',
    'log_lock_waits',
    'shared_preload_libraries',
    'pg_stat_statements.track',
    'track_io_timing'
)
ORDER BY name;

-- 8. Verify pg_stat_statements is working
SELECT count(*) as statement_count FROM pg_stat_statements;

-- =====================================================
-- IMPORTANT NOTES:
-- 
-- 1. If pg_stat_statements wasn't loaded, you need to restart PostgreSQL:
--    - Add 'pg_stat_statements' to shared_preload_libraries in postgresql.conf
--    - Restart the database
-- 
-- 2. After benchmarking, disable verbose logging:
--    ALTER SYSTEM SET log_min_duration_statement = 100;  -- only log queries > 100ms
--    ALTER SYSTEM SET log_statement = 'none';
--    SELECT pg_reload_conf();
--
-- 3. To analyze results after benchmark, use the analysis script
-- =====================================================

\echo 'PostgreSQL profiling setup complete!'
\echo 'Run your benchmark now.'
\echo 'Check if pg_stat_statements is loaded:'
SHOW shared_preload_libraries;
