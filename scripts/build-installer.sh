#!/usr/bin/env bash
# Build Abby installer and open the bundle folder.
# Run from repo root. Requires: Rust, Node.js 20+, npm.
set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$REPO_ROOT"

echo "Installing frontend deps (tauri-app/src-ui)..."
cd "$REPO_ROOT/tauri-app/src-ui"
npm install

echo "Building Tauri app (installer)..."
cd "$REPO_ROOT/tauri-app"
cargo tauri build

# Bundle output: workspace target or tauri-app/target
BUNDLE_NSIS="$REPO_ROOT/target/release/bundle/nsis"
if [ ! -d "$BUNDLE_NSIS" ]; then
  BUNDLE_NSIS="$REPO_ROOT/tauri-app/target/release/bundle/nsis"
fi
BUNDLE_DIR="$REPO_ROOT/target/release/bundle"
if [ ! -d "$BUNDLE_DIR" ]; then
  BUNDLE_DIR="$REPO_ROOT/tauri-app/target/release/bundle"
fi

echo "Opening bundle folder: $BUNDLE_DIR"
if command -v xdg-open >/dev/null 2>&1; then
  xdg-open "$BUNDLE_DIR"
elif command -v open >/dev/null 2>&1; then
  open "$BUNDLE_DIR"
else
  echo "Installers are in: $BUNDLE_DIR"
fi
