#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
VALIDATOR="$SCRIPT_DIR/validate_tauri_signing_key.sh"

pass_count=0

run_case() {
  local name="$1"
  local expect_exit="$2"
  local key="$3"
  local password="$4"

  set +e
  TAURI_SIGNING_PRIVATE_KEY="$key" \
  TAURI_SIGNING_PRIVATE_KEY_PASSWORD="$password" \
  DEBUG_LOG_PATH="/tmp/debug-9014be.log" \
  RUN_ID="test-$name" \
  bash "$VALIDATOR" >/tmp/validator-"$name".out 2>&1
  local rc=$?
  set -e

  if [[ "$rc" -ne "$expect_exit" ]]; then
    echo "FAIL: $name (expected exit $expect_exit, got $rc)"
    echo "---- output ----"
    cat /tmp/validator-"$name".out
    exit 1
  fi

  pass_count=$((pass_count + 1))
  echo "PASS: $name"
}

VALID_KEY=$'untrusted comment: minisign secret key\nRWQAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA'
VALID_ESCAPED='untrusted comment: minisign secret key\nRWQAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA'
INVALID_NO_COMMENT=$'RWQAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA'
INVALID_NO_RW=$'untrusted comment: minisign secret key\nNOT_A_MINISIGN_SECRET'

run_case "valid_raw_newlines" 0 "$VALID_KEY" "pw"
run_case "valid_escaped_newlines" 0 "$VALID_ESCAPED" "pw"
run_case "missing_comment" 1 "$INVALID_NO_COMMENT" "pw"
run_case "missing_rw_prefix" 1 "$INVALID_NO_RW" "pw"
run_case "missing_password" 1 "$VALID_KEY" ""

echo "All validator tests passed: $pass_count"
