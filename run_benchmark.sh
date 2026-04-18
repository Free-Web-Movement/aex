#!/bin/bash
# HTTP Benchmark: AEX vs Axum vs Simple

AEX_DIR="modules/aex"
cd "$AEX_DIR"

echo "=== Building benchmarks ==="
cargo build --release --example http_benchmark_aex
cargo build --release --example http_benchmark_axum  
cargo build --release --example simple_http_bench

echo ""
echo "=== Starting AEX server ==="
./target/release/examples/http_benchmark_aex &
AEX_PID=$!
sleep 2

echo "=== Running benchmarks ==="
echo ""
echo "--- AEX ---"
wrk -t4 -c100 -d10s http://127.0.0.1:8080/

echo ""
echo "--- Simple (baseline) ---"
pkill -f http_benchmark_aex || true
./target/release/examples/simple_http_bench &
sleep 2
wrk -t4 -c100 -d10s http://127.0.0.1:8080/

echo ""
echo "=== Cleanup ==="
pkill -f http_benchmark || true

echo "Done!"