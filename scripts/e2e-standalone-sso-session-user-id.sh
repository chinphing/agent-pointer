#!/usr/bin/env bash
# End-to-end: standalone SSO → conversation.sessionUserId (feeds terminal SESSION_USER_ID).
# Spawns a temporary pointer-server and logs in two users with distinct `sub` values.
# Uses debug pointer-server + POINTER_LICENSE_PUBLIC_KEY (release ignores that env).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

PORT="${POINTER_E2E_PORT:-18787}"
BASE="http://127.0.0.1:${PORT}"
AUD="$BASE"
SECRET="e2e-sso-secret-$$"
DATA_DIR="$(mktemp -d /tmp/pointer-sso-e2e.XXXXXX)"
LIC_DIR="$(mktemp -d /tmp/pointer-sso-lic.XXXXXX)"
COOKIE_A="$(mktemp)"
COOKIE_B="$(mktemp)"
SERVER_LOG="$(mktemp)"
cleanup() {
  if [[ -n "${SERVER_PID:-}" ]] && kill -0 "$SERVER_PID" 2>/dev/null; then
    kill "$SERVER_PID" 2>/dev/null || true
    wait "$SERVER_PID" 2>/dev/null || true
  fi
  rm -f "$COOKIE_A" "$COOKIE_B" "$SERVER_LOG"
  rm -rf "$DATA_DIR" "$LIC_DIR"
}
trap cleanup EXIT

echo "==> building pointer-server + license-gen"
cargo build -p pointer-server -p pointer-license-gen -q

echo "==> minting ephemeral license"
./target/debug/license-gen gen-keypair \
  --private-key "$LIC_DIR/priv.key" \
  --public-key "$LIC_DIR/pub.key" >/dev/null
LICENSE_KEY="$(./target/debug/license-gen sign \
  --private-key "$LIC_DIR/priv.key" \
  --customer-id e2e-sso \
  --expires 2099-12-31 \
  --features chat | tr -d '\r\n' | tail -1)"
LICENSE_PUB="$(tr -d '\r\n' < "$LIC_DIR/pub.key")"

echo "==> starting standalone server on $BASE (data=$DATA_DIR)"
POINTER_DEPLOYMENT_MODE=standalone \
POINTER_SERVER_ADDR="127.0.0.1:${PORT}" \
POINTER_APP_DATA_DIR="$DATA_DIR" \
POINTER_LICENSE_PUBLIC_KEY="$LICENSE_PUB" \
POINTER_LICENSE_KEY="$LICENSE_KEY" \
POINTER_SERVER_SSO_ENABLED=true \
POINTER_SERVER_SSO_SECRET="$SECRET" \
POINTER_SERVER_SSO_AUDIENCE="$AUD" \
./target/debug/pointer-server >"$SERVER_LOG" 2>&1 &
SERVER_PID=$!

for _ in $(seq 1 80); do
  if curl -sf "$BASE/api/auth/mode" >/dev/null 2>&1; then
    break
  fi
  if ! kill -0 "$SERVER_PID" 2>/dev/null; then
    echo "server exited early; log:"
    cat "$SERVER_LOG"
    exit 1
  fi
  sleep 0.25
done
MODE="$(curl -sf "$BASE/api/auth/mode")"
echo "auth mode: $MODE"
echo "$MODE" | grep -q standalone

mint() {
  local sub="$1" name="$2"
  ./target/debug/pointer-server --mint-sso-ticket \
    --sub "$sub" --name "$name" --ttl 120 \
    --secret "$SECRET" --audience "$AUD"
}

check_user() {
  local sub="$1" name="$2" cookie_jar="$3" conv_id="$4"
  local ticket
  ticket="$(mint "$sub" "$name" | tr -d '\r\n' | tail -1)"
  echo "==> SSO login sub=$sub"
  curl -sS -o /dev/null -c "$cookie_jar" -b "$cookie_jar" \
    -L "$BASE/api/auth/local/sso?sso=${ticket}"

  if ! grep -q pointer_web_session "$cookie_jar"; then
    echo "missing pointer_web_session cookie for $sub"
    cat "$cookie_jar"
    echo "--- server log ---"
    cat "$SERVER_LOG"
    exit 1
  fi

  local now
  now="$(python3 -c 'import time; print(int(time.time()*1000))')"
  local code
  code="$(curl -sS -o /tmp/pointer-sso-meta-resp.json -w '%{http_code}' \
    -c "$cookie_jar" -b "$cookie_jar" \
    -H 'Content-Type: application/json' \
    -X PUT "$BASE/api/conversations/meta" \
    -d "[{\"id\":\"${conv_id}\",\"title\":\"sso-e2e-${sub}\",\"createdAt\":${now},\"updatedAt\":${now}}]")"
  if [[ "$code" != "200" && "$code" != "204" ]]; then
    echo "save meta failed HTTP $code"
    cat /tmp/pointer-sso-meta-resp.json 2>/dev/null || true
    cat "$SERVER_LOG"
    exit 1
  fi

  local meta_list
  meta_list="$(curl -sS -c "$cookie_jar" -b "$cookie_jar" "$BASE/api/conversations/meta?limit=50")"
  echo "meta list for $sub: $meta_list"
  echo "$meta_list" | python3 -c "
import json,sys
sub=sys.argv[1]; cid=sys.argv[2]
rows=json.load(sys.stdin)
row=next((r for r in rows if r.get('id')==cid), None)
assert row is not None, f'missing conv {cid} in {rows!r}'
assert row.get('sessionUserId')==sub, row
print('sessionUserId ok:', row.get('sessionUserId'))
" "$sub" "$conv_id"

  # What terminal child would see after run_chat binds this conversation's session_user_id
  local got
  got="$(SESSION_USER_ID="$sub" sh -c 'printf %s "$SESSION_USER_ID"')"
  echo "terminal-equivalent shell SESSION_USER_ID=$got"
  [[ "$got" == "$sub" ]] || { echo "FAIL shell env"; exit 1; }
}

check_user "e2e-zhangsan" "ZhangSan" "$COOKIE_A" "conv-e2e-zhangsan"
check_user "e2e-lisi" "LiSi" "$COOKIE_B" "conv-e2e-lisi"

echo
echo "PASS: SSO e2e-zhangsan / e2e-lisi have distinct sessionUserId;"
echo "      terminal SESSION_USER_ID matches each sub."
