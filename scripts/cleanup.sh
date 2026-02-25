#!/usr/bin/env bash
# Cleanup generated artifacts and test outputs.
# Safe to run at any time — only removes build/test byproducts.
set -euo pipefail

echo "Cleaning frontend coverage outputs..."
rm -rf tauri-app/src-ui/coverage/

echo "Cleaning frontend build artifacts..."
rm -rf tauri-app/src-ui/dist/
rm -rf tauri-app/src-ui/node_modules/.vite

echo "Cleaning lcov/coverage data..."
find . -name "*.lcov" -delete 2>/dev/null || true
rm -f lcov.info

if [[ "${1:-}" == "--cargo-clean" ]]; then
    echo "Cleaning Rust build artifacts (cargo clean)..."
    cargo clean
fi

echo "Cleanup complete."
