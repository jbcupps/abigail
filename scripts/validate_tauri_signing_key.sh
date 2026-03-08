#!/usr/bin/env bash
set -euo pipefail

# Validate and sanitize TAURI_SIGNING_PRIVATE_KEY for the Tauri updater signer.
#
# This script:
#   1. Checks that TAURI_SIGNING_PRIVATE_KEY and PASSWORD are present
#   2. Accepts either Tauri 2 base64-encoded minisign secret boxes or raw multiline minisign boxes
#   3. Normalizes escaped newlines (\n -> real newlines) when raw multiline input is provided
#   4. Validates minisign structure (comment line + RW-prefixed base64)
#   5. Re-exports the sanitized key in the base64 format that Tauri expects

KEY_RAW="${TAURI_SIGNING_PRIVATE_KEY:-}"
KEY_PASSWORD="${TAURI_SIGNING_PRIVATE_KEY_PASSWORD:-}"

if [[ -z "$KEY_RAW" ]]; then
  echo "ERROR: TAURI_SIGNING_PRIVATE_KEY is empty or missing."
  exit 1
fi

if [[ -z "$KEY_PASSWORD" ]]; then
  echo "ERROR: TAURI_SIGNING_PRIVATE_KEY_PASSWORD is empty or missing."
  exit 1
fi

decode_base64_text() {
  local value="$1"
  local decoded

  if decoded="$(printf '%s' "$value" | base64 -d 2>/dev/null)"; then
    printf '%s' "$decoded"
    return 0
  fi

  if decoded="$(printf '%s' "$value" | base64 --decode 2>/dev/null)"; then
    printf '%s' "$decoded"
    return 0
  fi

  return 1
}

is_valid_base64() {
  local value="$1"

  if printf '%s' "$value" | base64 -d >/dev/null 2>&1; then
    return 0
  fi

  if printf '%s' "$value" | base64 --decode >/dev/null 2>&1; then
    return 0
  fi

  return 1
}

encode_base64() {
  printf '%s' "$1" | base64 | tr -d '\r\n'
}

validate_multiline_key() {
  local multiline_key="$1"
  local line1
  local line2

  line1="$(printf '%s\n' "$multiline_key" | sed -n '1p')"
  line2="$(printf '%s\n' "$multiline_key" | sed -n '2p' | tr -d '[:space:]')"

  if [[ "$line1" != untrusted\ comment:* ]]; then
    echo "ERROR: TAURI_SIGNING_PRIVATE_KEY first line must start with 'untrusted comment:'." >&2
    echo "       Got: ${line1:0:40}..." >&2
    return 1
  fi

  if [[ "$line2" != RW* ]]; then
    echo "ERROR: TAURI_SIGNING_PRIVATE_KEY second line must start with 'RW' (minisign secret key)." >&2
    echo "       Got: ${line2:0:20}..." >&2
    return 1
  fi

  if ! is_valid_base64 "$line2"; then
    echo "ERROR: TAURI_SIGNING_PRIVATE_KEY line 2 is not valid base64." >&2
    echo "       Please re-generate or re-copy your minisign secret key." >&2
    return 1
  fi

  printf '%s\n%s\n' "$line1" "$line2"
}

KEY_MULTILINE=""
KEY_SANITIZED_BASE64=""

if [[ "$KEY_RAW" == *$'\n'* || "$KEY_RAW" == *'\n'* || "$KEY_RAW" == untrusted\ comment:* ]]; then
  KEY_MULTILINE="$KEY_RAW"
  if [[ "$KEY_MULTILINE" == *'\n'* ]]; then
    KEY_MULTILINE="$(printf '%b' "${KEY_MULTILINE//\\n/$'\n'}")"
    echo "INFO: Normalized escaped newlines in signing key."
  fi

  KEY_MULTILINE="$(validate_multiline_key "$KEY_MULTILINE")"
  KEY_SANITIZED_BASE64="$(encode_base64 "$KEY_MULTILINE")"
  echo "INFO: Converted raw minisign secret key to Tauri base64 format."
else
  KEY_SANITIZED_BASE64="$(printf '%s' "$KEY_RAW" | tr -d '[:space:]')"
  if [[ -z "$KEY_SANITIZED_BASE64" ]]; then
    echo "ERROR: TAURI_SIGNING_PRIVATE_KEY is empty after whitespace normalization."
    exit 1
  fi

  if ! KEY_MULTILINE="$(decode_base64_text "$KEY_SANITIZED_BASE64")"; then
    echo "ERROR: TAURI_SIGNING_PRIVATE_KEY is not valid base64."
    echo "       Tauri 2 expects a base64-encoded minisign secret key box."
    exit 1
  fi

  KEY_MULTILINE="$(validate_multiline_key "$KEY_MULTILINE")"
  KEY_SANITIZED_BASE64="$(encode_base64 "$KEY_MULTILINE")"
  echo "INFO: Base64 signing key validation passed."
fi

if [[ -n "${GITHUB_ENV:-}" ]]; then
  {
    echo "TAURI_SIGNING_PRIVATE_KEY<<EOF"
    printf '%s\n' "$KEY_SANITIZED_BASE64"
    echo "EOF"
  } >> "$GITHUB_ENV"
  echo "INFO: Exported sanitized signing key to GITHUB_ENV."
fi

LINE_COUNT="$(printf '%s\n' "$KEY_MULTILINE" | wc -l | tr -d ' ')"
echo "Signing key preflight passed (lines: $LINE_COUNT)."
