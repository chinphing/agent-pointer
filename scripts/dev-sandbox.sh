#!/usr/bin/env bash
# 本地 E2B 沙箱开发环境（e2b-local + OrbStack）
#
# 用途：**只用于验证 envd 协议**（remote_env_it 集成测试）。
#       日常开发已改用 docker 后端（POINTER_SERVER_ENVIRONMENT_BACKEND=docker），
#       不再需要这个网关——它挂不了项目目录（卷只能是 ~/.e2b-local/volumes 下的真实目录）。
#       控制面与数据面均为真实 E2B 协议（envd 为 E2B 官方二进制）。
#
# 用法：
#   scripts/dev-sandbox.sh up       启动网关并等待健康
#   scripts/dev-sandbox.sh down     停止并清理网关
#   scripts/dev-sandbox.sh status   查看状态
#   scripts/dev-sandbox.sh smoke    冒烟测试（建沙箱 + exec + 读写文件）
#   scripts/dev-sandbox.sh bench    跑基准脚本（需 python3）
#   scripts/dev-sandbox.sh clean    清理残留的沙箱容器
#
# 环境变量：
#   E2B_LOCAL_PORT   网关端口（默认 3000）
#   E2B_LOCAL_IMAGE  镜像（默认 ghcr.io/superduck-ai/e2b-local:latest）

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
WORK_DIR="${REPO_ROOT}/.dev-sandbox"
ENVD_DIR="${WORK_DIR}/envd-bin"
CONFIG="${WORK_DIR}/config.yaml"
PORT="${E2B_LOCAL_PORT:-3000}"
IMAGE="${E2B_LOCAL_IMAGE:-ghcr.io/superduck-ai/e2b-local:latest}"
CONTAINER="pointer-e2b-local"

log()  { printf '\033[36m[dev-sandbox]\033[0m %s\n' "$*"; }
warn() { printf '\033[33m[dev-sandbox]\033[0m %s\n' "$*" >&2; }
die()  { printf '\033[31m[dev-sandbox]\033[0m %s\n' "$*" >&2; exit 1; }

require_orbstack() {
  command -v docker >/dev/null 2>&1 || die "未找到 docker"
  local os
  os="$(docker info --format '{{.OperatingSystem}}' 2>/dev/null || true)"
  case "$os" in
    OrbStack*) return 0 ;;
    "") die "Docker 未运行。先启动 OrbStack：open /Applications/OrbStack.app" ;;
    *)  die "当前 Docker 上下文是「${os}」，不是 OrbStack。
     e2b-local 需要 host 网络模式，Docker Desktop 的 --network host 是 VM netns，
     不转发到 macOS，网关无法访问沙箱端口。
     切换：docker context use orbstack" ;;
  esac
}

envd_binary_name() {
  case "$(uname -m)" in
    arm64|aarch64) echo envd-linux-arm64 ;;
    x86_64|amd64)  echo envd-linux-amd64 ;;
    *) die "不支持的架构：$(uname -m)" ;;
  esac
}

cmd_up() {
  require_orbstack
  mkdir -p "$WORK_DIR"

  if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
    log "拉取镜像 $IMAGE"
    docker pull "$IMAGE"
  fi

  local bin; bin="$(envd_binary_name)"
  if [[ ! -x "${ENVD_DIR}/${bin}" ]]; then
    log "提取 envd 二进制（镜像自带）"
    mkdir -p "$ENVD_DIR"
    docker run --rm -v "${ENVD_DIR}:/out" --entrypoint sh "$IMAGE" \
      -c 'cp -r /app/envd-bin/. /out/ && chmod +x /out/*'
  fi

  # 卷目录必须放在**宿主可见**路径：docker daemon 解析 bind 源路径时看不到
  # 网关容器内部的 /root/.e2b-local，只有挂成宿主同路径才通。
  # 同时把整个 ~/.e2b-local 挂进网关：卷目录若是符号链接，网关要能 stat 到目标目录
  # （否则报 "docker local volume path ... is not a directory"）。
  local volumes_dir; volumes_dir="${E2B_LOCAL_VOLUMES_DIR:-${HOME}/.e2b-local/volumes}"
  mkdir -p "$volumes_dir"

  cat > "$CONFIG" <<EOF
traffic:
  advertised_host: "127.0.0.1"
docker:
  envd_binary: "${ENVD_DIR}/${bin}"
  volume_host_path: "${volumes_dir}"
EOF

  docker rm -f "$CONTAINER" >/dev/null 2>&1 || true
  log "启动网关（host 网络）"
  docker run -d --name "$CONTAINER" --network host \
    -v "${HOME}/.orbstack/run/docker.sock:/var/run/docker.sock" \
    -v "${ENVD_DIR}:${ENVD_DIR}:ro" \
    -v "${HOME}/.e2b-local:${HOME}/.e2b-local" \
    -v "${CONFIG}:/config.yaml:ro" \
    --entrypoint /usr/local/bin/e2b-local \
    "$IMAGE" serve --config /config.yaml >/dev/null

  # 关键：先确认容器真的活着，否则健康检查可能被「端口上的其他进程」骗过
  sleep 1
  if [[ "$(docker inspect -f '{{.State.Running}}' "$CONTAINER" 2>/dev/null)" != "true" ]]; then
    docker logs "$CONTAINER" 2>&1 | tail -15 >&2
    if lsof -nP -iTCP:"${PORT}" -sTCP:LISTEN >/dev/null 2>&1; then
      warn "端口 ${PORT} 已被占用："
      lsof -nP -iTCP:"${PORT}" -sTCP:LISTEN 2>/dev/null | sed -n '2,3p' >&2
      die "请先释放端口，或用 E2B_LOCAL_PORT=其他端口 启动"
    fi
    die "容器启动后立即退出"
  fi

  for _ in $(seq 1 30); do
    if curl -fsS -m 2 "http://127.0.0.1:${PORT}/health" >/dev/null 2>&1; then
      log "就绪：http://127.0.0.1:${PORT}"
      return 0
    fi
    sleep 1
  done
  docker logs "$CONTAINER" 2>&1 | tail -20 >&2
  die "网关 30 秒内未就绪"
}

cmd_clean() {
  local n
  n="$(docker ps -aq --filter 'name=e2b-envd-' | wc -l | tr -d ' ')"
  if [[ "$n" != "0" ]]; then
    docker rm -f $(docker ps -aq --filter 'name=e2b-envd-') >/dev/null 2>&1 || true
    log "清理了 ${n} 个残留沙箱容器"
  else
    log "无残留沙箱容器"
  fi
}

cmd_down() {
  docker rm -f "$CONTAINER" >/dev/null 2>&1 && log "已停止" || log "未在运行"
}

cmd_status() {
  local os; os="$(docker info --format '{{.OperatingSystem}}' 2>/dev/null || echo unknown)"
  log "docker runtime: ${os}"
  if docker ps --filter "name=${CONTAINER}" --format '{{.Status}}' | grep -q .; then
    local st; st="$(docker ps --filter "name=${CONTAINER}" --format '{{.Status}}')"
    log "网关容器: ${st}"
    curl -fsS -m 3 "http://127.0.0.1:${PORT}/health" >/dev/null 2>&1 \
      && log "health: OK" || warn "health: 不可达"
  else
    log "网关容器: 未运行"
  fi
}

cmd_smoke() {
  require_orbstack
  python3 - "$PORT" <<'PY'
import base64, json, sys, urllib.request

base = f"http://127.0.0.1:{sys.argv[1]}"

def post(path, obj=None, root=base, ctype="application/json", raw=None):
    data = raw if raw is not None else json.dumps(obj).encode()
    req = urllib.request.Request(root + path, data=data, headers={"Content-Type": ctype})
    return urllib.request.urlopen(req, timeout=120).read()

def get(path, root=base):
    return urllib.request.urlopen(root + path, timeout=120).read()

def exec_cmd(envd, cmd):
    payload = json.dumps({"process": {"cmd": "/bin/sh", "args": ["-c", cmd]}}).encode()
    body = b"\x00" + len(payload).to_bytes(4, "big") + payload
    req = urllib.request.Request(
        envd + "/process.Process/Start", data=body,
        headers={"Content-Type": "application/connect+json"})
    raw = urllib.request.urlopen(req, timeout=120).read()
    out, i = [], 0
    while i + 5 <= len(raw):
        flags = raw[i]; ln = int.from_bytes(raw[i + 1:i + 5], "big")
        seg = raw[i + 5:i + 5 + ln]; i += 5 + ln
        if flags & 0x02:
            break
        try:
            ev = json.loads(seg).get("event", {})
        except Exception:
            continue
        if "data" in ev and "stdout" in ev["data"]:
            out.append(base64.b64decode(ev["data"]["stdout"]).decode(errors="replace"))
    return "".join(out).strip()

info = json.loads(post("/sandboxes", {"templateID": "debian:bookworm-slim", "timeout": 300}))
envd, sid = info["envdURL"], info["sandboxID"]
print(f"  sandbox={sid}  envd={envd}")
try:
    print(f"  exec -> {exec_cmd(envd, 'echo ok && uname -m')}")
    post("/files?path=/tmp/smoke.txt&username=root", None, root=envd,
         ctype="application/octet-stream", raw=b"hello")
    content = get("/files?path=/tmp/smoke.txt&username=root", root=envd).decode()
    print(f"  file -> {content}")
    assert content == "hello", "文件读写不一致"
    print("  冒烟通过")
finally:
    try:
        urllib.request.urlopen(urllib.request.Request(
            f"{base}/sandboxes/{sid}", method="DELETE"), timeout=30)
    except Exception:
        pass
PY
}

cmd_bench() {
  local script="${REPO_ROOT}/docs/zh-CN/design/bench_e2b.py"
  [[ -f "$script" ]] || die "找不到 ${script}"
  python3 "$script"
}

case "${1:-}" in
  up)     cmd_up ;;
  down)   cmd_down ;;
  status) cmd_status ;;
  smoke)  cmd_smoke ;;
  bench)  cmd_bench ;;
  clean)  cmd_clean ;;
  *)      sed -n '2,20p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'; exit 1 ;;
esac
