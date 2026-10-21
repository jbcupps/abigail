#!/usr/bin/env bash
# LEGACY: builds the retired single-agent tauri-app, not the Abigail framework
# installer. Use scripts/build-mvp-windows.ps1 for the family-facing product.
# Requires: Rust, Node.js 20+, npm.
set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
LEGACY_APP_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
REPO_ROOT="$(cd "$LEGACY_APP_DIR/../.." && pwd)"

echo "Installing frontend deps (legacy/tauri-app/src-ui)..."
cd "$LEGACY_APP_DIR/src-ui"
npm install

echo "Building legacy Tauri app (installer)..."
cd "$LEGACY_APP_DIR"
cargo tauri build

# Generate the Hive master key (idempotent - won't overwrite existing)
echo "Generating Hive master key..."
cd "$REPO_ROOT"
cargo run -p abigail-keygen -- --gen-master || echo "Master key already exists or generation skipped."

BUNDLE_DIR="$LEGACY_APP_DIR/target/release/bundle"
if [ -d "$BUNDLE_DIR/nsis" ]; then
  BUNDLE_DIR="$BUNDLE_DIR/nsis"
fi

echo "Opening bundle folder: $BUNDLE_DIR"
if command -v xdg-open >/dev/null 2>&1; then
  xdg-open "$BUNDLE_DIR"
elif command -v open >/dev/null 2>&1; then
  open "$BUNDLE_DIR"
else
  echo "Installers are in: $BUNDLE_DIR"
fi
