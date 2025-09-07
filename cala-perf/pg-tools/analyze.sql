-- =====================================================
-- PostgreSQL Performance Analysis Script - Fixed Version
-- Works with PostgreSQL 14+
-- =====================================================

\echo '========================================='
\echo 'TOP 10 SLOWEST QUERIES BY TOTAL TIME'
\echo '========================================='

SELECT 
    substring(query, 1, 100) as query_preview,
    calls,
    round(total_exec_time::numeric, 2) as total_ms,
    CASE 
        WHEN calls > 0 THEN round((total_exec_time::numeric/calls), 2) 
        ELSE 0 
    END as avg_ms,
    round(stddev_exec_time::numeric, 2) as stddev_ms,
    round(min_exec_time::numeric, 2) as min_ms,
    round(max_exec_time::numeric, 2) as max_ms,
    rows,
    CASE 
        WHEN sum(total_exec_time) over () > 0 
        THEN round((100.0 * total_exec_time / sum(total_exec_time) over ())::numeric, 2)
        ELSE 0
    END as percentage_of_total
FROM pg_stat_statements
WHERE query NOT LIKE '%pg_stat_statements%'
ORDER BY total_exec_time DESC
LIMIT 10;

\echo ''
\echo '========================================='
\echo 'TOP 10 MOST FREQUENT QUERIES'
\echo '========================================='

SELECT 
    substring(query, 1, 100) as query_preview,
    calls,
    round(total_exec_time::numeric, 2) as total_ms,
    CASE 
        WHEN calls > 0 THEN round((total_exec_time::numeric/calls), 2) 
        ELSE 0 
    END as avg_ms,
    rows,
    CASE 
        WHEN calls > 0 THEN round((rows::numeric/calls), 2) 
        ELSE 0 
    END as avg_rows
FROM pg_stat_statements
WHERE query NOT LIKE '%pg_stat_statements%'
ORDER BY calls DESC
LIMIT 10;

\echo ''
\echo '========================================='
\echo 'QUERIES WITH HIGH STANDARD DEVIATION'
\echo '========================================='

SELECT 
    substring(query, 1, 100) as query_preview,
    calls,
    round(mean_exec_time::numeric, 2) as avg_ms,
    round(stddev_exec_time::numeric, 2) as stddev_ms,
    CASE
        WHEN mean_exec_time > 0 THEN round((stddev_exec_time / mean_exec_time)::numeric, 2)
        ELSE 0
    END as coefficient_of_variation
FROM pg_stat_statements
WHERE calls > 10  -- Only queries called multiple times
    AND query NOT LIKE '%pg_stat_statements%'
ORDER BY stddev_exec_time DESC
LIMIT 10;

\echo ''
\echo '========================================='
\echo 'DATABASE WAIT EVENTS SUMMARY'
\echo '========================================='

SELECT 
    wait_event_type,
    wait_event,
    count(*) as occurrences
FROM pg_stat_activity
WHERE wait_event IS NOT NULL
GROUP BY wait_event_type, wait_event
ORDER BY count(*) DESC;

\echo ''
\echo '========================================='
\echo 'TABLE I/O STATISTICS'
\echo '========================================='

-- Using explicit column selection that works in PG 14+
SELECT 
    t.schemaname,
    t.relname as tablename,
    t.seq_scan,
    t.seq_tup_read,
    t.idx_scan,
    t.idx_tup_fetch,
    t.n_tup_ins,
    t.n_tup_upd,
    t.n_tup_del,
    t.n_tup_hot_upd,
    t.n_live_tup,
    t.n_dead_tup
FROM pg_stat_all_tables t
WHERE t.schemaname = 'public'
ORDER BY COALESCE(t.seq_tup_read, 0) + COALESCE(t.idx_tup_fetch, 0) DESC
LIMIT 10;

\echo ''
\echo '========================================='
\echo 'INDEX USAGE STATISTICS'
\echo '========================================='

SELECT 
    s.schemaname,
    s.relname as tablename,
    s.indexrelname as indexname,
    s.idx_scan,
    s.idx_tup_read,
    s.idx_tup_fetch,
    CASE 
        WHEN s.idx_scan = 0 THEN 'UNUSED INDEX - Consider dropping'
        WHEN s.idx_scan < 50 THEN 'RARELY USED - Review necessity'
        ELSE 'Active'
    END as status
FROM pg_stat_all_indexes s
WHERE s.schemaname = 'public'
ORDER BY s.idx_scan ASC
LIMIT 20;

\echo ''
\echo '========================================='
\echo 'MISSING INDEX OPPORTUNITIES'
\echo '========================================='

SELECT 
    t.schemaname,
    t.relname as tablename,
    t.seq_scan,
    t.seq_tup_read,
    CASE 
        WHEN t.seq_scan > 0 THEN t.seq_scan * t.seq_tup_read
        ELSE 0
    END AS inefficiency_score,
    pg_size_pretty(pg_relation_size(t.schemaname||'.'||t.relname)) as table_size
FROM pg_stat_all_tables t
WHERE t.schemaname = 'public' 
    AND t.seq_scan > 100  -- Tables with many sequential scans
    AND t.seq_tup_read > 1000  -- That read many tuples
ORDER BY (t.seq_scan * t.seq_tup_read) DESC
LIMIT 10;

\echo ''
\echo '========================================='
\echo 'CONNECTION POOL ANALYSIS'
\echo '========================================='

SELECT 
    state,
    count(*) as connection_count,
    max(now() - state_change) as longest_in_state,
    avg(now() - state_change) as avg_time_in_state
FROM pg_stat_activity
GROUP BY state
ORDER BY connection_count DESC;

\echo ''
\echo '========================================='
\echo 'QUERY SUMMARY BY TYPE'
\echo '========================================='

SELECT 
    CASE 
        WHEN query LIKE 'SELECT%' THEN 'SELECT'
        WHEN query LIKE 'INSERT%' THEN 'INSERT'
        WHEN query LIKE 'UPDATE%' THEN 'UPDATE'
        WHEN query LIKE 'DELETE%' THEN 'DELETE'
        WHEN query LIKE 'WITH%' THEN 'CTE'
        WHEN query LIKE 'BEGIN%' OR query LIKE 'COMMIT%' OR query LIKE 'ROLLBACK%' THEN 'TRANSACTION'
        ELSE 'OTHER'
    END as query_type,
    COUNT(*) as unique_queries,
    SUM(calls) as total_calls,
    ROUND(SUM(total_exec_time)::numeric, 2) as total_time_ms,
    ROUND(AVG(mean_exec_time)::numeric, 2) as avg_time_per_call
FROM pg_stat_statements
GROUP BY query_type
ORDER BY total_time_ms DESC;

\echo ''
\echo '========================================='
\echo 'EXPORTING FULL QUERY DETAILS'
\echo '========================================='

-- Export full details to CSV for analysis
\copy (SELECT queryid, query, calls, total_exec_time, mean_exec_time, stddev_exec_time, rows FROM pg_stat_statements ORDER BY total_exec_time DESC) TO '/tmp/pg_query_stats.csv' WITH CSV HEADER;

\echo ''
\echo 'Full query statistics exported to: /tmp/pg_query_stats.csv'
\echo ''
\echo '========================================='
\echo 'TOP 5 QUERIES TO OPTIMIZE'
\echo '========================================='

SELECT 
    substring(query, 1, 60) as query_preview,
    calls,
    round(total_exec_time::numeric, 2) as total_ms,
    round(mean_exec_time::numeric, 2) as avg_ms,
    CASE 
        WHEN total_exec_time > 1000 THEN 'HIGH IMPACT - Optimize query'
        WHEN calls > 10000 AND mean_exec_time > 1 THEN 'HIGH FREQUENCY - Consider caching'
        WHEN stddev_exec_time > mean_exec_time THEN 'INCONSISTENT - Check plan stability'
        ELSE 'Monitor'
    END as recommendation
FROM pg_stat_statements
WHERE query NOT LIKE '%pg_stat_statements%'
    AND calls > 0
ORDER BY total_exec_time DESC
LIMIT 5;

\echo ''
\echo '========================================='
\echo 'RESET INSTRUCTIONS'
\echo '========================================='
\echo 'To reset statistics for next benchmark run:'
\echo '  SELECT pg_stat_statements_reset();'
\echo ''
\echo 'To disable verbose logging after analysis:'
\echo '  ALTER SYSTEM SET log_min_duration_statement = 100;'
\echo '  ALTER SYSTEM SET log_statement = ''none'';'
\echo '  SELECT pg_reload_conf();'
