-- =====================================================
-- PostgreSQL Performance Analysis - Simplified Version
-- =====================================================

\echo '========================================='
\echo 'CHECKING PG_STAT_STATEMENTS STATUS'
\echo '========================================='

SELECT count(*) as total_queries_tracked FROM pg_stat_statements;

\echo ''
\echo '========================================='
\echo 'TOP 10 QUERIES BY TOTAL TIME (SIMPLIFIED)'
\echo '========================================='

SELECT 
    LEFT(query, 80) as query_preview,
    calls,
    total_exec_time::bigint as total_ms,
    (total_exec_time/calls)::bigint as avg_ms,
    rows
FROM pg_stat_statements
WHERE query NOT LIKE '%pg_stat_statements%'
ORDER BY total_exec_time DESC
LIMIT 10;

\echo ''
\echo '========================================='
\echo 'TOP 10 MOST FREQUENT QUERIES'
\echo '========================================='

SELECT 
    LEFT(query, 80) as query_preview,
    calls,
    total_exec_time::bigint as total_ms,
    rows
FROM pg_stat_statements
WHERE query NOT LIKE '%pg_stat_statements%'
ORDER BY calls DESC
LIMIT 10;

\echo ''
\echo '========================================='
\echo 'QUERIES GROUPED BY TYPE'
\echo '========================================='

SELECT 
    CASE 
        WHEN query LIKE 'SELECT%' THEN 'SELECT'
        WHEN query LIKE 'INSERT%' THEN 'INSERT'
        WHEN query LIKE 'UPDATE%' THEN 'UPDATE'
        WHEN query LIKE 'DELETE%' THEN 'DELETE'
        WHEN query LIKE 'BEGIN%' OR query LIKE 'COMMIT%' OR query LIKE 'ROLLBACK%' THEN 'TRANSACTION'
        ELSE 'OTHER'
    END as query_type,
    COUNT(*) as query_count,
    SUM(calls) as total_calls,
    SUM(total_exec_time)::bigint as total_time_ms
FROM pg_stat_statements
GROUP BY query_type
ORDER BY total_time_ms DESC;

\echo ''
\echo '========================================='
\echo 'DATABASE ACTIVITY CHECK'
\echo '========================================='

SELECT 
    datname,
    numbackends as active_connections,
    xact_commit as transactions_committed,
    xact_rollback as transactions_rolled_back,
    tup_returned as tuples_returned,
    tup_fetched as tuples_fetched,
    tup_inserted as tuples_inserted,
    tup_updated as tuples_updated,
    tup_deleted as tuples_deleted
FROM pg_stat_database
WHERE datname = current_database();

\echo ''
\echo '========================================='
\echo 'FULL QUERY LIST (First 20)'
\echo '========================================='

SELECT 
    queryid,
    calls,
    total_exec_time::bigint as total_ms,
    mean_exec_time::bigint as avg_ms,
    LEFT(query, 150) as query
FROM pg_stat_statements
ORDER BY total_exec_time DESC
LIMIT 20;

\echo ''
\echo '========================================='
\echo 'To see the full CSV export:'
\echo 'cat /tmp/pg_query_stats.csv'
\echo '========================================='
