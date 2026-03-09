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

normalize_windows_signing_mode() {
  local value="${1:-}"
  value="$(printf '%s' "$value" | tr '[:upper:]' '[:lower:]')"
  value="${value//[[:space:]]/}"
  case "$value" in
    ""|off|false|none)
      printf '%s' ""
      ;;
    pfx|store)
      printf '%s' "$value"
      ;;
    *)
      echo "ERROR: Unsupported ABIGAIL_WINDOWS_SIGNING_MODE '$value'." >&2
      exit 1
      ;;
  esac
}

require_updater_signing="${ABIGAIL_REQUIRE_UPDATER_SIGNING:-${ABIGAIL_OFFICIAL_RELEASE:-false}}"
require_windows_signing="${ABIGAIL_REQUIRE_WINDOWS_SIGNING:-false}"
require_mac_signing="${ABIGAIL_REQUIRE_MAC_SIGNING:-false}"
windows_signing_mode="$(normalize_windows_signing_mode "${ABIGAIL_WINDOWS_SIGNING_MODE:-}")"

if ! is_truthy "$require_updater_signing" && \
   ! is_truthy "$require_windows_signing" && \
   ! is_truthy "$require_mac_signing"; then
  echo "Release prerequisite enforcement skipped (no signing requirements enabled)."
  exit 0
fi

if is_truthy "$require_updater_signing"; then
  require_var TAURI_SIGNING_PRIVATE_KEY
  require_var TAURI_SIGNING_PRIVATE_KEY_PASSWORD
  require_var TAURI_UPDATER_PUBKEY
fi

if is_truthy "$require_windows_signing"; then
  require_var WINDOWS_CERTIFICATE_THUMBPRINT
  require_var WINDOWS_TIMESTAMP_URL
  if [[ -z "$windows_signing_mode" ]]; then
    if [[ -n "${WINDOWS_SIGNING_CERT_BASE64:-}" || -n "${WINDOWS_SIGNING_CERT_PASSWORD:-}" ]]; then
      windows_signing_mode="pfx"
    else
      echo "ERROR: ABIGAIL_WINDOWS_SIGNING_MODE must be set to 'store' for hardware-token signing or 'pfx' for exportable certificate signing." >&2
      exit 1
    fi
  fi

  case "$windows_signing_mode" in
    pfx)
      require_var WINDOWS_SIGNING_CERT_BASE64
      require_var WINDOWS_SIGNING_CERT_PASSWORD
      ;;
    store)
      require_var ABIGAIL_WINDOWS_RUNNER
      ;;
  esac
fi

if is_truthy "$require_mac_signing"; then
  require_var APPLE_CERTIFICATE
  require_var APPLE_CERTIFICATE_PASSWORD
  require_var APPLE_SIGNING_IDENTITY
  require_var APPLE_ID
  require_var APPLE_PASSWORD
  require_var APPLE_TEAM_ID
fi

echo "Release prerequisite check passed."
