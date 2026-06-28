#!/bin/bash
# HTTP QPS Benchmark: AEX vs Axum
# Usage: ./bench_qps.sh [duration_seconds]
# Requires: wrk

set -euo pipefail

DURATION="${1:-10}"
MODULE_DIR="$(cd "$(dirname "$0")" && pwd)"
WORKSPACE="$(cd "$MODULE_DIR/../.." && pwd)"
BIN="$WORKSPACE/target/release/examples"
AEX_PORT=8080
AXUM_PORT=8081

echo ""
echo "========================================"
echo "  HTTP QPS Benchmark: AEX vs Axum"
echo "  Duration: ${DURATION}s per test"
echo "========================================"

# ─── Build ────────────────────────────────────────────────────────────────

echo ""
echo ">>> Building..."
cargo build --release --example http_benchmark_aex 2>&1 | grep -E "Compiling|Finished|error" | sed 's/^/  /'
cargo build --release --example http_benchmark_axum 2>&1 | grep -E "Compiling|Finished|error" | sed 's/^/  /'

if ! command -v wrk &>/dev/null; then
    echo "ERROR: wrk not found (apt install wrk)"
    exit 1
fi

# ─── Helpers ──────────────────────────────────────────────────────────────

kill_pid() {
    local pid=$1
    [ -n "$pid" ] && kill "$pid" 2>/dev/null || true
}

qps()     { grep "Requests/sec" | awk '{print $2}'; }
lat_avg() { grep "Latency" | head -1 | awk '{print $2}'; }
lat_p99() { grep "99%" | awk '{print $2}'; }

# ─── Start Servers ────────────────────────────────────────────────────────

echo ""
echo ">>> Starting AEX on port $AEX_PORT..."
"$BIN/http_benchmark_aex" &
AEX_PID=$!
sleep 3
if ! kill -0 $AEX_PID 2>/dev/null; then
    echo "ERROR: AEX failed to start"
    kill_pid "$AXUM_PID"
    exit 1
fi
echo "  AEX PID=$AEX_PID"

echo ""
echo ">>> Starting Axum on port $AXUM_PORT..."
"$BIN/http_benchmark_axum" &
AXUM_PID=$!
sleep 3
if ! kill -0 $AXUM_PID 2>/dev/null; then
    echo "ERROR: Axum failed to start"
    kill_pid "$AEX_PID"
    exit 1
fi
echo "  Axum PID=$AXUM_PID"

# ─── Warmup ───────────────────────────────────────────────────────────────

echo ""
echo ">>> Warming up..."
wrk -t2 -c10 -d3s http://127.0.0.1:$AEX_PORT/ >/dev/null 2>&1 || true
wrk -t2 -c10 -d3s http://127.0.0.1:$AXUM_PORT/ >/dev/null 2>&1 || true
sleep 1

# ─── Run Benchmarks ────────────────────────────────────────────────────────

run_bench() {
    local label=$1 port=$2 url=$3 threads=$4 conn=$5
    wrk -t"$threads" -c"$conn" -d"${DURATION}s" \
        "http://127.0.0.1:$port$url" 2>/dev/null
}

result() {
    local f=$(mktemp); cat > "$f"; echo "$f"
}

table_row() {
    local url=$1 t=$2 c=$3
    local aex_out=$(mktemp); run_bench "AEX $url" $AEX_PORT "$url" "$t" "$c" > "$aex_out"
    local axum_out=$(mktemp); run_bench "Axum $url" $AXUM_PORT "$url" "$t" "$c" > "$axum_out"

    local aqps=$(qps < "$aex_out")
    local axqps=$(qps < "$axum_out")
    local al99=$(lat_p99 < "$aex_out")
    local axl99=$(lat_p99 < "$axum_out")

    # Default to 0 if empty
    aqps="${aqps:-0}"
    axqps="${axqps:-0}"

    local ratio
    ratio=$(echo "scale=2; $aqps / $axqps" | bc 2>/dev/null || echo "?")

    printf "t%-1d-c%-4d │ %-20s │ %10s │ %10s │ %6s │ %s / %s\n" \
        "$t" "$c" "$url" \
        "$(printf '%.0f' "$aqps")" \
        "$(printf '%.0f' "$axqps")" \
        "$ratio" \
        "$al99" "$axl99"

    rm -f "$aex_out" "$axum_out"
}

# ─── Print Results ────────────────────────────────────────────────────────

echo ""
echo "═══════════════════════════════════════════════════════════════════"
echo "  QPS COMPARISON"
echo "═══════════════════════════════════════════════════════════════════"
printf "%-7s │ %-20s │ %10s │ %10s │ %6s │ %s\n" \
    "Clients" "Route" "AEX QPS" "Axum QPS" "Ratio" "P99 Lat"
printf "%0.s─" $(seq 1 80)
echo ""

# Test scenarios: (threads, connections)
for tc in "2 10" "4 100" "8 200" "4 500"; do
    read -r t c <<< "$tc"
    for url in "/" "/api/users" "/api/users/123"; do
        table_row "$url" "$t" "$c"
    done
done

# ─── Best QPS ─────────────────────────────────────────────────────────────

printf "%0.s─" $(seq 1 80)
echo ""

# Run all combos for / to find best QPS
AEX_BEST=0; AXUM_BEST=0
for tc in "2 10" "4 100" "8 200" "4 500"; do
    read -r t c <<< "$tc"
    for port_var in "$AEX_PORT" "$AXUM_PORT"; do
        out=$(run_bench "best" "$port_var" "/" "$t" "$c")
        q=$(qps <<< "$out")
        q="${q:-0}"
        if [ "$port_var" = "$AEX_PORT" ]; then
            AEX_BEST=$(echo "if ($q > $AEX_BEST) $q else $AEX_BEST" | bc 2>/dev/null)
        else
            AXUM_BEST=$(echo "if ($q > $AXUM_BEST) $q else $AXUM_BEST" | bc 2>/dev/null)
        fi
    done
done

RATIO=$(echo "scale=2; $AEX_BEST / $AXUM_BEST" | bc 2>/dev/null || echo "?")

printf "%-7s │ %-20s │ %10s │ %10s │ %6s\n" \
    "BEST" "(root)" \
    "$(printf '%.0f' "$AEX_BEST")" \
    "$(printf '%.0f' "$AXUM_BEST")" \
    "x${RATIO}"

echo ""
printf "  AEX %s x Axum on best QPS\n" "$RATIO"

# ─── Cleanup ──────────────────────────────────────────────────────────────

kill_pid "$AEX_PID"
kill_pid "$AXUM_PID"
echo ""
echo "  Done."
