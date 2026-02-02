# test_build.sh

#!/usr/bin/env bash

set -e

sudo apt install gcc 

# Enable Rust coverage instrumentation
export RUSTFLAGS="-C instrument-coverage -Awarnings"
export LLVM_PROFILE_FILE=".output/cargo-test-%p-%m.profraw"

# Run tests
echo "Running cargo tests with coverage instrumentation..."
cargo test --tests -- --nocapture --show-output

# Find all .profraw files
PROFRAW_FILES=$(find . -wholename ".output/*.profraw")

# Generate coverage report if grcov is installed
rm -rf coverages || true

check_installed() {
    if ! command -v "$1" > /dev/null 2>&1 
    then
    echo "Installing $1"
    cargo install $1
    fi
}

check_installed grcov
check_installed cargo-llvm-cov
check_installed cargo-nextest

# Generate coverage report
echo "Generating coverage report with grcov..."
cargo llvm-cov nextest --html --output-dir coverages

# Clean up .profraw files
rm -rf .output/
echo "Test and coverage script completed."

# Show result in browser


if command -v xdg-open > /dev/null 2>&1 
then
    xdg-open coverages/html/index.html
fi

