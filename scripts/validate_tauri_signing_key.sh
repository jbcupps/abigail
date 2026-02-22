#!/usr/bin/env bash
set -euo pipefail

# Validate and sanitize TAURI_SIGNING_PRIVATE_KEY for the Tauri updater signer.
#
# This script:
#   1. Checks that TAURI_SIGNING_PRIVATE_KEY and PASSWORD are present
#   2. Normalizes escaped newlines (\n → real newlines)
#   3. Strips stray whitespace from the base64 key line
#   4. Validates minisign structure (comment line + RW-prefixed base64)
#   5. Validates that the key line is legal base64
#   6. Exports the cleaned key via GITHUB_ENV so the build step uses it

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

# --- Normalize escaped newlines ---
KEY_NORMALIZED="$KEY_RAW"
if [[ "$KEY_RAW" == *'\n'* ]]; then
  KEY_NORMALIZED="$(printf '%b' "${KEY_RAW//\\n/$'\n'}")"
  echo "INFO: Normalized escaped newlines in signing key."
fi

# --- Extract and validate structure ---
LINE1="$(printf '%s\n' "$KEY_NORMALIZED" | sed -n '1p')"
LINE2="$(printf '%s\n' "$KEY_NORMALIZED" | sed -n '2p')"
LINE_COUNT="$(printf '%s\n' "$KEY_NORMALIZED" | wc -l | tr -d ' ')"

if [[ "$LINE1" != untrusted\ comment:* ]]; then
  echo "ERROR: TAURI_SIGNING_PRIVATE_KEY first line must start with 'untrusted comment:'."
  echo "       Got: ${LINE1:0:40}..."
  exit 1
fi

if [[ "$LINE2" != RW* ]]; then
  echo "ERROR: TAURI_SIGNING_PRIVATE_KEY second line must start with 'RW' (minisign secret key)."
  echo "       Got: ${LINE2:0:20}..."
  exit 1
fi

# --- Strip stray whitespace from the base64 key line ---
LINE2_CLEAN="$(echo "$LINE2" | tr -d '[:space:]')"
if [[ "$LINE2_CLEAN" != "$LINE2" ]]; then
  echo "WARNING: Stripped whitespace from base64 key line (likely copy-paste artifact)."
  # Rebuild the full key with the cleaned line 2
  KEY_NORMALIZED="$(printf '%s\n%s' "$LINE1" "$LINE2_CLEAN")"
fi

# --- Validate base64 encoding ---
# The key line should be valid base64. Use base64 decode as a check.
if command -v base64 &>/dev/null; then
  if ! echo "$LINE2_CLEAN" | base64 -d >/dev/null 2>&1 && \
     ! echo "$LINE2_CLEAN" | base64 --decode >/dev/null 2>&1; then
    echo "ERROR: TAURI_SIGNING_PRIVATE_KEY line 2 is not valid base64."
    echo "       Please re-generate or re-copy your minisign secret key."
    exit 1
  fi
  echo "INFO: Base64 validation passed."
fi

# --- Export cleaned key for subsequent steps ---
if [[ -n "${GITHUB_ENV:-}" ]]; then
  {
    echo "TAURI_SIGNING_PRIVATE_KEY<<EOF"
    printf '%s\n' "$KEY_NORMALIZED"
    echo "EOF"
  } >> "$GITHUB_ENV"
  echo "INFO: Exported sanitized signing key to GITHUB_ENV."
fi

echo "Signing key preflight passed (lines: $LINE_COUNT)."
