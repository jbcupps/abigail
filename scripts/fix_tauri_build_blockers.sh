#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

echo "[1/7] Installing Linux desktop build dependencies (required for Tauri/WebKit on Ubuntu)..."
if command -v apt-get >/dev/null 2>&1; then
  sudo apt-get update
  sudo apt-get install -y \
    pkg-config \
    libglib2.0-dev \
    libgtk-3-dev \
    libwebkit2gtk-4.1-dev \
    libsoup-3.0-dev \
    libayatana-appindicator3-dev
fi

echo "[2/7] Installing cargo-tauri CLI when missing..."
if ! cargo tauri --version >/dev/null 2>&1; then
  cargo install cargo-tauri --locked
fi

echo "[3/7] Ensuring Playwright driver version is available at Rust compile time..."
export PLAYWRIGHT_DRIVER_VERSION="${PLAYWRIGHT_DRIVER_VERSION:-1.56.1}"

echo "[4/7] Installing frontend dependencies and production assets..."
(
  cd tauri-app/src-ui
  npm install
  npm run build
)

echo "[5/7] Installing Playwright Chromium for runtime browser skill support..."
(
  cd tauri-app/src-ui
  npx playwright@"$PLAYWRIGHT_DRIVER_VERSION" install chromium
)

echo "[6/7] Validating Rust workspace compiles..."
cargo check --workspace

echo "[7/7] Validating Tauri desktop build..."
(
  cd tauri-app
  cargo tauri build --no-bundle
)

echo "Build blockers addressed and validations completed."
