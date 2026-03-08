#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
VALIDATOR="$SCRIPT_DIR/validate_tauri_signing_key.sh"

pass_count=0

decode_base64() {
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

run_case() {
  local name="$1"
  local expect_exit="$2"
  local key="$3"
  local password="$4"
  local expected_export="${5:-}"
  local env_path="/tmp/validator-$name.env"

  rm -f "$env_path"
  set +e
  TAURI_SIGNING_PRIVATE_KEY="$key" \
  TAURI_SIGNING_PRIVATE_KEY_PASSWORD="$password" \
  GITHUB_ENV="$env_path" \
  bash "$VALIDATOR" >/tmp/validator-"$name".out 2>&1
  local rc=$?
  set -e

  if [[ "$rc" -ne "$expect_exit" ]]; then
    echo "FAIL: $name (expected exit $expect_exit, got $rc)"
    echo "---- output ----"
    cat /tmp/validator-"$name".out
    exit 1
  fi

  if [[ "$expect_exit" -eq 0 && -n "$expected_export" ]]; then
    if [[ ! -f "$env_path" ]]; then
      echo "FAIL: $name (validator did not export GITHUB_ENV)"
      exit 1
    fi

    local exported
    exported="$(awk '/^TAURI_SIGNING_PRIVATE_KEY<<EOF$/{flag=1;next}/^EOF$/{flag=0}flag' "$env_path")"
    if [[ "$exported" != "$expected_export" ]]; then
      echo "FAIL: $name (unexpected exported key)"
      echo "Expected: $expected_export"
      echo "Actual:   $exported"
      exit 1
    fi
  fi

  pass_count=$((pass_count + 1))
  echo "PASS: $name"
}

VALID_BASE64='dW50cnVzdGVkIGNvbW1lbnQ6IHJzaWduIGVuY3J5cHRlZCBzZWNyZXQga2V5ClJXUlRZMEl5dkpDN09RZm5GeVAzc2RuYlNzWVVJelJRQnNIV2JUcGVXZUplWXZXYXpqUUFBQkFBQUFBQUFBQUFBQUlBQUFBQTZrN2RnWGh5dURxSzZiL1ZQSDdNcktiaHRxczQwMXdQelRHbjRNcGVlY1BLMTBxR2dpa3I3dDE1UTVDRDE4MXR4WlQwa1BQaXdxKy9UU2J2QmVSNXhOQWFDeG1GSVllbUNpTGJQRkhhTnROR3I5RmdUZi90OGtvaGhJS1ZTcjdZU0NyYzhQWlQ5cGM9Cg=='
VALID_KEY="$(decode_base64 "$VALID_BASE64")"
VALID_CANONICAL_BASE64="$(printf '%s' "$VALID_KEY" | base64 | tr -d '\r\n')"
VALID_ESCAPED="${VALID_KEY//$'\n'/\\n}"
INVALID_NO_COMMENT=$'RWQAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA'
INVALID_NO_RW=$'untrusted comment: minisign secret key\nNOT_A_MINISIGN_SECRET'
INVALID_BASE64='not-base64'

run_case "valid_base64" 0 "$VALID_BASE64" "pw" "$VALID_CANONICAL_BASE64"
run_case "valid_raw_newlines" 0 "$VALID_KEY" "pw" "$VALID_CANONICAL_BASE64"
run_case "valid_escaped_newlines" 0 "$VALID_ESCAPED" "pw" "$VALID_CANONICAL_BASE64"
run_case "missing_comment" 1 "$INVALID_NO_COMMENT" "pw"
run_case "missing_rw_prefix" 1 "$INVALID_NO_RW" "pw"
run_case "invalid_base64" 1 "$INVALID_BASE64" "pw"
run_case "missing_password" 1 "$VALID_KEY" ""

echo "All validator tests passed: $pass_count"
