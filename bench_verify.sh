#!/bin/bash
# 独立复验脚本 - 每次只启动一个服务器，交替顺序，输出完整wrk数据
# Usage: ./bench_verify.sh

set -euo pipefail

MODULE_DIR="$(cd "$(dirname "$0")" && pwd)"
WORKSPACE="$(cd "$MODULE_DIR/../.." && pwd)"
BIN="$WORKSPACE/target/release/examples"

cleanup() {
    for p in "$@"; do kill "$p" 2>/dev/null || true; done
    sleep 1
}

echo ""
echo "══════════════════════════════════════════════════════"
echo "  独立复验：AEX vs Axum QPS"
echo "  每次只启动一个服务器，交替顺序"
echo "══════════════════════════════════════════════════════"
echo ""

# Test AEX alone
echo "=== [1/2] 测试 AEX (无Axum干扰) ==="
cargo build --release --example http_benchmark_aex 2>&1 | tail -1
"$BIN/http_benchmark_aex" > /dev/null 2>&1 &
AEX_PID=$!
sleep 3

if kill -0 $AEX_PID 2>/dev/null; then
    echo "--- AEX / (4t-100c-5s) ---"
    wrk -t4 -c100 -d5s --latency http://127.0.0.1:8080/
    echo ""
    echo "--- AEX /api/users ---"
    wrk -t4 -c100 -d5s --latency http://127.0.0.1:8080/api/users
    echo ""
    echo "--- AEX /api/users/123 ---"
    wrk -t4 -c100 -d5s --latency http://127.0.0.1:8080/api/users/123
else
    echo "AEX failed to start!"
fi
cleanup "$AEX_PID"

# Test Axum alone
echo ""
echo "=== [2/2] 测试 Axum (无AEX干扰) ==="
cargo build --release --example http_benchmark_axum 2>&1 | tail -1
"$BIN/http_benchmark_axum" > /dev/null 2>&1 &
AXUM_PID=$!
sleep 3

if kill -0 $AXUM_PID 2>/dev/null; then
    echo "--- Axum / (4t-100c-5s) ---"
    wrk -t4 -c100 -d5s --latency http://127.0.0.1:8081/
    echo ""
    echo "--- Axum /api/users ---"
    wrk -t4 -c100 -d5s --latency http://127.0.0.1:8081/api/users
    echo ""
    echo "--- Axum /api/users/123 ---"
    wrk -t4 -c100 -d5s --latency http://127.0.0.1:8081/api/users/123
else
    echo "Axum failed to start!"
fi
cleanup "$AXUM_PID"

echo ""
echo "══════════════════════════════════════════════════════"
echo "  复验完成，请对比以上数据"
echo "══════════════════════════════════════════════════════"
