#!/usr/bin/env bash

set -e

# Check for output file argument
if [ $# -eq 0 ]; then
    echo "Usage: $0 <output-file>"
    echo "Example: $0 perf-report.md"
    exit 1
fi

OUTPUT_FILE="$1"

# Change to the project root directory
cd "$(dirname "$0")/.."

make reset-deps

cargo bench -p cala-perf
# Run load tests and extract just the summary table
load_output=$(cargo run -p cala-perf 2>&1)

{
echo "## 🚀 Cala Performance Benchmark Results (non-representative)"

echo "### 🏃 Criterion Benchmark Results (singe-threaded)"
echo ""

echo "| Benchmark | Time per Run | Throughput | % vs Baseline |"
echo "|-----------|--------------|------------|---------------|"

# Parse the generated JSON files in target/criterion/
baseline_time=""

# Find all estimates.json files in target/criterion subdirectories (in the 'new' subdirectory)
for json_file in target/criterion/*/new/estimates.json; do
    if [[ -f "$json_file" ]]; then
        # Extract benchmark name from path (parent of the 'new' directory)
        bench_name=$(basename "$(dirname "$(dirname "$json_file")")")

        # Extract the mean estimate (in nanoseconds)
        time_ns=$(jq -r '.mean.point_estimate' "$json_file")

        if [[ -n "$time_ns" && "$time_ns" != "null" ]]; then
            # Convert nanoseconds to milliseconds
            time_ms=$(echo "scale=3; $time_ns / 1000000" | bc -l)
            time_display="${time_ms}ms"

            # Convert to tx/s (assuming single operation per benchmark)
            tx_per_sec=$(echo "scale=0; 1000000000 / $time_ns" | bc -l)

            # Calculate percentage difference from baseline
            if [[ -z "$baseline_time" ]]; then
                baseline_time=$time_ns
                perc_diff="0 (baseline)"
            else
                perc_diff=$(echo "scale=2; ($time_ns - $baseline_time) / $baseline_time * 100" | bc -l | xargs printf "%.1f")
                # Flip the sign: slower (higher time) = negative %, faster (lower time) = positive %
                if (( $(echo "$perc_diff >= 0" | bc -l) )); then
                    perc_diff="-${perc_diff}%"
                else
                    perc_diff="+${perc_diff#-}%"
                fi
            fi

            echo "| ${bench_name#* } | $time_display | ${tx_per_sec} tx/s | $perc_diff |"
        fi
    fi
done

echo ""
echo "### 🏋️ Load Testing Results (parallel-execution)"
echo ""

# Extract the summary table section, skip the header and separator lines
echo "$load_output" | sed -n '/📋 PERFORMANCE SUMMARY TABLE/,/✅ All performance tests completed!/p' | sed '$d' | sed '1,2d'

echo "---"
echo ""
echo "💡 **Note**: Performance results may vary based on system resources and database state."
echo "These benchmarks help identify performance trends and potential bottlenecks."

} > "$OUTPUT_FILE"

echo "Performance report generated: $OUTPUT_FILE"
