#!/usr/bin/env bash

set -e

# Change to the project root directory
cd "$(dirname "$0")/.."

echo "## 🚀 Cala Performance Benchmark Results"
echo ""
echo "Running performance tests..."
echo ""

# Run the performance tests and capture output
cargo run -p cala-perf 2>&1 | sed 's/^/    /'

echo ""
echo "---"
echo ""
echo "💡 **Note**: Performance results may vary based on system resources and database state."
echo "These benchmarks help identify performance trends and potential bottlenecks."
