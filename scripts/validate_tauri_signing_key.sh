#!/usr/bin/env bash
set -euo pipefail

DEBUG_LOG_PATH="${DEBUG_LOG_PATH:-debug-9014be.log}"
DEBUG_SESSION_ID="9014be"
RUN_ID="${RUN_ID:-pre-fix}"

log_debug() {
  local hypothesis_id="$1"
  local location="$2"
  local message="$3"
  local data_json="$4"
  local ts
  ts="$(python - <<'PY'
import time
print(int(time.time() * 1000))
PY
)"
  # #region agent log
  printf '{"sessionId":"%s","runId":"%s","hypothesisId":"%s","location":"%s","message":"%s","data":%s,"timestamp":%s}\n' \
    "$DEBUG_SESSION_ID" "$RUN_ID" "$hypothesis_id" "$location" "$message" "$data_json" "$ts" >> "$DEBUG_LOG_PATH"
  # #endregion
}

KEY_RAW="${TAURI_SIGNING_PRIVATE_KEY:-}"
KEY_PASSWORD="${TAURI_SIGNING_PRIVATE_KEY_PASSWORD:-}"

if [[ -z "$KEY_RAW" ]]; then
  log_debug "H2" "scripts/validate_tauri_signing_key.sh:24" "missing key secret" '{"hasKey":false}'
  echo "ERROR: TAURI_SIGNING_PRIVATE_KEY is empty or missing."
  exit 1
fi

if [[ -z "$KEY_PASSWORD" ]]; then
  log_debug "H3" "scripts/validate_tauri_signing_key.sh:30" "missing key password" '{"hasPassword":false}'
  echo "ERROR: TAURI_SIGNING_PRIVATE_KEY_PASSWORD is empty or missing."
  exit 1
fi

KEY_NORMALIZED="$KEY_RAW"
if [[ "$KEY_RAW" == *'\n'* ]]; then
  KEY_NORMALIZED="$(printf '%b' "${KEY_RAW//\\n/$'\n'}")"
  log_debug "H1" "scripts/validate_tauri_signing_key.sh:38" "normalized escaped newlines" '{"hadEscapedNewlines":true}'
else
  log_debug "H1" "scripts/validate_tauri_signing_key.sh:40" "key already had raw newlines" '{"hadEscapedNewlines":false}'
fi

LINE1="$(printf '%s\n' "$KEY_NORMALIZED" | sed -n '1p')"
LINE2="$(printf '%s\n' "$KEY_NORMALIZED" | sed -n '2p')"
LINE_COUNT="$(printf '%s\n' "$KEY_NORMALIZED" | wc -l | tr -d ' ')"

HAS_COMMENT=false
HAS_RW_PREFIX=false
[[ "$LINE1" == untrusted\ comment:* ]] && HAS_COMMENT=true
[[ "$LINE2" == RW* ]] && HAS_RW_PREFIX=true

log_debug "H2" "scripts/validate_tauri_signing_key.sh:52" "validated structural lines" "{\"lineCount\":$LINE_COUNT,\"hasComment\":$HAS_COMMENT,\"hasRwPrefix\":$HAS_RW_PREFIX}"

if [[ "$HAS_COMMENT" != "true" ]]; then
  echo "ERROR: TAURI_SIGNING_PRIVATE_KEY first line must start with 'untrusted comment:'."
  exit 1
fi

if [[ "$HAS_RW_PREFIX" != "true" ]]; then
  log_debug "H4" "scripts/validate_tauri_signing_key.sh:60" "second line missing RW prefix" '{"isLikelyWrongKeyType":true}'
  echo "ERROR: TAURI_SIGNING_PRIVATE_KEY second line must look like minisign secret key data (starts with 'RW')."
  exit 1
fi

if [[ -n "${GITHUB_ENV:-}" ]]; then
  {
    echo "TAURI_SIGNING_PRIVATE_KEY_NORMALIZED<<EOF"
    printf '%s\n' "$KEY_NORMALIZED"
    echo "EOF"
  } >> "$GITHUB_ENV"
fi

log_debug "H4" "scripts/validate_tauri_signing_key.sh:74" "key validation passed" '{"passed":true}'
echo "Signing key preflight passed."
