#!/bin/bash
# Usage: ./scripts/benchmark/axum.sh [duration_seconds] [threads] [connections] [url]
set -euo pipefail

MODULE_DIR="$(cd "$(dirname "$0")/../.." && pwd)"
WORKSPACE="$(cd "$MODULE_DIR/../.." && pwd)"
BIN="$WORKSPACE/target/release/examples"

DURATION="${1:-10}"
THREADS="${2:-4}"
CONNECTIONS="${3:-100}"
URL="${4:-/}"

echo ">>> Building Axum server..."
cargo build --release --example http_benchmark_axum 2>&1 | grep -E "Compiling|Finished"

echo ">>> Starting Axum on 127.0.0.1:8081..."
"$BIN/http_benchmark_axum" > /dev/null 2>&1 &
PID=$!
sleep 3

if ! kill -0 $PID 2>/dev/null; then
    echo "ERROR: Axum failed to start"
    exit 1
fi

echo ">>> wrk -t$THREADS -c$CONNECTIONS -d${DURATION}s http://127.0.0.1:8081$URL"
wrk -t"$THREADS" -c"$CONNECTIONS" -d"${DURATION}s" --latency "http://127.0.0.1:8081$URL"

kill $PID 2>/dev/null
echo "Done."
