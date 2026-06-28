#!/bin/bash
# Usage: ./scripts/benchmark/aex.sh [duration_seconds] [threads] [connections] [url]
set -euo pipefail

MODULE_DIR="$(cd "$(dirname "$0")/../.." && pwd)"
WORKSPACE="$(cd "$MODULE_DIR/../.." && pwd)"
BIN="$WORKSPACE/target/release/examples"

DURATION="${1:-10}"
THREADS="${2:-4}"
CONNECTIONS="${3:-100}"
URL="${4:-/}"

echo ">>> Building AEX server..."
cargo build --release --example http_benchmark_aex 2>&1 | grep -E "Compiling|Finished"

echo ">>> Starting AEX on 127.0.0.1:8080..."
"$BIN/http_benchmark_aex" > /dev/null 2>&1 &
PID=$!
sleep 3

if ! kill -0 $PID 2>/dev/null; then
    echo "ERROR: AEX failed to start"
    exit 1
fi

echo ">>> wrk -t$THREADS -c$CONNECTIONS -d${DURATION}s http://127.0.0.1:8080$URL"
wrk -t"$THREADS" -c"$CONNECTIONS" -d"${DURATION}s" --latency "http://127.0.0.1:8080$URL"

kill $PID 2>/dev/null
echo "Done."
