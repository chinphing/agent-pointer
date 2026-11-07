#!/bin/bash
# 控制面端到端冒烟：起进程 → 建资源 → OAuth 授权 → 取凭证 → 上报用量。
# 用法：先 cargo build -p pointer-admin，然后在仓库根目录执行 bash admin/scripts/smoke.sh
set -u
BASE=${BASE:-http://127.0.0.1:8899}
DB=${DB:-/tmp/pointer-admin-smoke.db}
CJ=${CJ:-/tmp/pointer-admin-cj.txt}
rm -f "$DB" "$CJ"

export POINTER_ADMIN_ADDR=127.0.0.1:8899
export POINTER_ADMIN_DB_URL="sqlite:$DB"
export POINTER_ADMIN_BOOTSTRAP_USER=admin
export POINTER_ADMIN_BOOTSTRAP_PASSWORD=admin12345
export POINTER_ADMIN_OAUTH_SECRET=s3cret
export POINTER_ADMIN_SERVER_REDIRECT=http://127.0.0.1:8787/auth/callback
export RUST_LOG=warn

./target/debug/pointer-admin >/tmp/pointer-admin.log 2>&1 &
PID=$!
trap 'kill $PID 2>/dev/null' EXIT
sleep 2

jqp() { python3 -c "import sys,json;d=json.load(sys.stdin);print(eval(sys.argv[1]))" "$1"; }

echo "== 1. 管理员登录 =="
curl -s -c "$CJ" -X POST "$BASE/api/admin/auth/login" -H 'content-type: application/json' \
  -d '{"username":"admin","password":"admin12345"}' | head -c 300; echo

echo "== 2. 建 provider（带密钥）=="
PROV=$(curl -s -b "$CJ" -X POST "$BASE/api/admin/resources" -H 'content-type: application/json' \
  -d '{"kind":"provider","slug":"qwen","name":"阿里云百炼","payload":{"baseUrl":"https://example.invalid/v1"},"secret":"sk-live","enabled":true}')
echo "$PROV" | head -c 300; echo
PROV_ID=$(echo "$PROV" | jqp "d['id']")

echo "== 3. 建 model（挂在 provider 下）=="
MODEL=$(curl -s -b "$CJ" -X POST "$BASE/api/admin/resources" -H 'content-type: application/json' \
  -d "{\"kind\":\"model\",\"slug\":\"qwen3.5-plus\",\"name\":\"qwen3.5-plus\",\"parentId\":\"$PROV_ID\",\"enabled\":true}")
MODEL_ID=$(echo "$MODEL" | jqp "d['id']")
echo "model id=$MODEL_ID"

echo "== 4. 授权 all =="
for ID in "$PROV_ID" "$MODEL_ID"; do
  curl -s -b "$CJ" -X PUT "$BASE/api/admin/resources/$ID/grants" -H 'content-type: application/json' \
    -d '{"items":[{"type":"all","id":""}]}'; echo
done

echo "== 5. 授权页可打开 =="
VERIFIER=verifier-123
CHALLENGE=$(printf '%s' "$VERIFIER" | openssl dgst -binary -sha256 | openssl base64 -A | tr '+/' '-_' | tr -d '=')
echo "challenge=$CHALLENGE"
curl -s -o /dev/null -w 'GET /oauth/authorize -> %{http_code}\n' \
  "$BASE/oauth/authorize?client_id=pointer-server&redirect_uri=http://127.0.0.1:8787/auth/callback&code_challenge=$CHALLENGE&code_challenge_method=S256&state=xyz"

echo "== 6. 提交登录，拿授权码 =="
LOC=$(curl -s -o /dev/null -D - -X POST "$BASE/oauth/authorize" \
  -d "username=admin&password=admin12345&client_id=pointer-server&redirect_uri=http://127.0.0.1:8787/auth/callback&state=xyz&code_challenge=$CHALLENGE" \
  | tr -d '\r' | awk 'tolower($1)=="location:"{print $2}')
echo "redirect=$LOC"
CODE=$(echo "$LOC" | sed -n 's/.*code=\([a-f0-9]*\).*/\1/p')
echo "code=$CODE"

echo "== 7. exchange-code =="
EX=$(curl -s -X POST "$BASE/auth/oauth/exchange-code" -H 'content-type: application/json' \
  -d "{\"client_id\":\"pointer-server\",\"client_secret\":\"s3cret\",\"code\":\"$CODE\",\"code_verifier\":\"$VERIFIER\"}")
echo "$EX" | head -c 600; echo
TOKEN=$(echo "$EX" | jqp "d['access_token']")

echo "== 8. partner/llm-credentials =="
curl -s "$BASE/auth/partner/llm-credentials" -H "Authorization: Bearer $TOKEN" | head -c 600; echo

echo "== 9. 授权码不能重放 =="
curl -s -o /dev/null -w 'replay -> %{http_code}\n' -X POST "$BASE/auth/oauth/exchange-code" -H 'content-type: application/json' \
  -d "{\"client_id\":\"pointer-server\",\"client_secret\":\"s3cret\",\"code\":\"$CODE\",\"code_verifier\":\"$VERIFIER\"}"

echo "== 10. 无 token 访问 partner =="
curl -s -o /dev/null -w 'no-token -> %{http_code}\n' "$BASE/auth/partner/llm-credentials"

echo "== 11. 用量上报 + 总览 =="
curl -s -X POST "$BASE/auth/partner/token-usage" -H "Authorization: Bearer $TOKEN" \
  -H 'content-type: application/json' -d '{"model":"qwen3.5-plus","prompt_tokens":120,"completion_tokens":80}'; echo
curl -s -b "$CJ" "$BASE/api/admin/overview"; echo

echo "== 12. 审计 =="
curl -s -b "$CJ" "$BASE/api/admin/audit?limit=3" | head -c 500; echo

echo "== 13. 前端产物 =="
INDEX=$(curl -s "$BASE/")
if echo "$INDEX" | grep -q 'id="app"'; then
  echo "index.html -> 已挂载前端产物"
  ASSET=$(echo "$INDEX" | grep -o '/assets/[^"]*\.js' | head -1)
  curl -s -o /dev/null -w "asset $ASSET -> %{http_code}\n" "$BASE$ASSET"
else
  echo "index.html -> 占位页（未执行 npm run build）"
fi
