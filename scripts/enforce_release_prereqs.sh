#!/usr/bin/env bash
set -euo pipefail

is_truthy() {
  local value="${1:-}"
  [[ "$value" =~ ^([Tt][Rr][Uu][Ee]|[Yy][Ee][Ss]|1)$ ]]
}

require_var() {
  local name="$1"
  if [[ -z "${!name:-}" ]]; then
    echo "ERROR: ${name} is required for this release build."
    exit 1
  fi
}

if ! is_truthy "${ABIGAIL_OFFICIAL_RELEASE:-false}"; then
  echo "Release prerequisite enforcement skipped (ABIGAIL_OFFICIAL_RELEASE is false)."
  exit 0
fi

require_var TAURI_SIGNING_PRIVATE_KEY
require_var TAURI_SIGNING_PRIVATE_KEY_PASSWORD
require_var TAURI_UPDATER_PUBKEY

if is_truthy "${ABIGAIL_REQUIRE_WINDOWS_SIGNING:-false}"; then
  require_var WINDOWS_SIGNING_CERT_BASE64
  require_var WINDOWS_SIGNING_CERT_PASSWORD
  require_var WINDOWS_CERTIFICATE_THUMBPRINT
  require_var WINDOWS_TIMESTAMP_URL
fi

if is_truthy "${ABIGAIL_REQUIRE_MAC_SIGNING:-false}"; then
  require_var APPLE_CERTIFICATE
  require_var APPLE_CERTIFICATE_PASSWORD
  require_var APPLE_SIGNING_IDENTITY
  require_var APPLE_ID
  require_var APPLE_PASSWORD
  require_var APPLE_TEAM_ID
fi

echo "Release prerequisite check passed."
