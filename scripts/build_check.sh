#!/usr/bin/env bash
set -euo pipefail

echo "=== Checking workspace ==="
cargo check --workspace

echo "=== Running tests ==="
cargo test --workspace

echo "=== Build complete ==="
echo "All checks passed!"