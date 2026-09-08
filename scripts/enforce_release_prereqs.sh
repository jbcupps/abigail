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
    pfx|store|esigner)
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
  if [[ "$windows_signing_mode" != "esigner" ]]; then
    echo "ERROR: Signed split-product releases require ABIGAIL_WINDOWS_SIGNING_MODE=esigner and the verified build_signed_installer.ps1 pipeline." >&2
    echo "The legacy store/PFX release path does not verify the extracted offline installer payload." >&2
    exit 1
  fi
  require_var WINDOWS_CERTIFICATE_THUMBPRINT
  require_var ESIGNER_USERNAME
  require_var ESIGNER_PASSWORD
  require_var ESIGNER_CREDENTIAL_ID
  require_var ESIGNER_TOTP_SECRET
  if is_truthy "$require_updater_signing"; then
    echo "ERROR: The eSigner NSIS lane does not yet generate updater artifacts. Disable ABIGAIL_REQUIRE_UPDATER_SIGNING." >&2
    exit 1
  fi
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
