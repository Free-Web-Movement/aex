#!/bin/bash
# HTTP Benchmark: AEX vs Axum vs Actix-web vs Simple

WORKSPACE="/home/eric/Projects/FreeWebMovement/zz-rust-mod-crypto-currency"
BIN="$WORKSPACE/target/release/examples"

echo "=== Building all benchmarks ==="
cd "$WORKSPACE"
cargo build --release --example http_benchmark_aex 2>/dev/null
cargo build --release --example http_benchmark_axum 2>/dev/null
cargo build --release --example http_benchmark_actix 2>/dev/null
cargo build --release --example simple_http_bench 2>/dev/null

echo ""
echo "═══════════════════════════════════════════════════════════════════════════════"
echo "                        BENCHMARK RESULTS"
echo "═══════════════════════════════════════════════════════════════════════════════"

# Helper function to run benchmark and extract RPS
run_bench() {
    local name=$1
    local port=$2
    local url=$3
    
    echo ""
    echo "--- $name ($url) ---"
    wrk -t4 -c100 -d5s "http://127.0.0.1:$port$url" 2>&1 | grep -E "Requests/sec|Latency"
}

# ============================================================================
# Test 1: No URL (/)
# ============================================================================
echo ""
echo "═══════════════════════════════════════════════════════════════════════════════"
echo "                      1. NO URL (/)"
echo "═══════════════════════════════════════════════════════════════════════════════"

# Simple baseline
pkill -f simple_http 2>/dev/null || true
sleep 1
"$BIN/simple_http_bench" &
sleep 2
run_bench "Simple (baseline)" 8080 "/"
pkill -f simple_http

# AEX
sleep 1
"$BIN/http_benchmark_aex" &
sleep 2
run_bench "AEX" 8080 "/"
pkill -f http_benchmark_aex

# Axum
sleep 1
"$BIN/http_benchmark_axum" &
sleep 2
run_bench "Axum" 8081 "/"
pkill -f http_benchmark_axum

# Actix-web
sleep 1
"$BIN/http_benchmark_actix" &
sleep 2
run_bench "Actix-web" 8082 "/"
pkill -f http_benchmark_actix

# ============================================================================
# Test 2: Static URL (/api/users)
# ============================================================================
echo ""
echo "═══════════════════════════════════════════════════════════════════════════════"
echo "                    2. STATIC URL (/api/users)"
echo "═══════════════════════════════════════════════════════════════════════════════"

pkill -f simple_http 2>/dev/null || true
sleep 1
"$BIN/http_benchmark_aex" &
sleep 2
run_bench "AEX" 8080 "/api/users"
pkill -f http_benchmark_aex

sleep 1
"$BIN/http_benchmark_axum" &
sleep 2
run_bench "Axum" 8081 "/api/users"
pkill -f http_benchmark_axum

sleep 1
"$BIN/http_benchmark_actix" &
sleep 2
run_bench "Actix-web" 8082 "/api/users"
pkill -f http_benchmark_actix

# ============================================================================
# Test 3: Dynamic URL (/api/users/123)
# ============================================================================
echo ""
echo "═══════════════════════════════════════════════════════════════════════════════"
echo "                  3. DYNAMIC URL (/api/users/:id)"
echo "═══════════════════════════════════════════════════════════════════════════════"

sleep 1
"$BIN/http_benchmark_aex" &
sleep 2
run_bench "AEX" 8080 "/api/users/123"
pkill -f http_benchmark_aex

sleep 1
"$BIN/http_benchmark_axum" &
sleep 2
run_bench "Axum" 8081 "/api/users/123"
pkill -f http_benchmark_axum

sleep 1
"$BIN/http_benchmark_actix" &
sleep 2
run_bench "Actix-web" 8082 "/api/users/123"
pkill -f http_benchmark_actix

echo ""
echo "═══════════════════════════════════════════════════════════════════════════════"
echo "Done!"