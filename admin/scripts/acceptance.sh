#!/bin/bash
# 本地验收环境：起一个带演示数据的控制面，给人工点界面用。
#
# 为什么要有这个脚本：这些环境变量散在命令行里，重启时漏一个就会出现
# 「概览页报未配置 POINTER_ADMIN_OAUTH_SECRET」或者「起来是个空库」这种假故障。
# 统一从这里起，就不会再漏。
#
#   bash admin/scripts/acceptance.sh start      # 起（已有进程会先停）
#   bash admin/scripts/acceptance.sh stop
#   bash admin/scripts/acceptance.sh status
#   bash admin/scripts/acceptance.sh reset      # 删库重来（要重新 seed）
#
# 账号：admin / Admin@12345（首次启动用 bootstrap 建，已存在则忽略）
set -u

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
DIR="${ACCEPTANCE_DIR:-$HOME/.pointer-admin-acceptance}"
PORT="${PORT:-8788}"
BIN="$ROOT/target/debug/pointer-admin"

export POINTER_ADMIN_ADDR="127.0.0.1:$PORT"
export POINTER_ADMIN_DB_URL="sqlite:$DIR/admin.db"
export POINTER_ADMIN_DATA_DIR="$DIR/data"
export POINTER_ADMIN_BOOTSTRAP_USER=admin
export POINTER_ADMIN_BOOTSTRAP_PASSWORD=Admin@12345
# 客户端（pointer-server / 桌面端）换凭证要用的共享密钥。本地验收用固定值，
# 部署到真环境必须换成随机串并同步到客户端配置。
export POINTER_ADMIN_OAUTH_SECRET="${POINTER_ADMIN_OAUTH_SECRET:-acceptance-oauth-secret}"
export POINTER_ADMIN_SERVER_REDIRECT="${POINTER_ADMIN_SERVER_REDIRECT:-http://127.0.0.1:8787/auth/callback}"
# 验证码默认开着，和人工验收时看到的一致；跑脚本化的验收再传 CAPTCHA=off。
export POINTER_ADMIN_CAPTCHA="${CAPTCHA:-on}"
export RUST_LOG="${RUST_LOG:-info}"

status() {
  if curl -s -o /dev/null --max-time 1 "http://127.0.0.1:$PORT/"; then
    echo "运行中：http://127.0.0.1:$PORT"
    curl -s "http://127.0.0.1:$PORT/api/admin/auth/captcha" | head -c 40; echo
  else
    echo "未运行"
  fi
}

stop() {
  pkill -f 'target/debug/pointer-admin' 2>/dev/null
  sleep 1
}

start() {
  stop
  mkdir -p "$DIR/data"
  if [ ! -x "$BIN" ]; then
    echo "找不到 $BIN，先跑 cargo build -p pointer-admin"
    exit 1
  fi
  nohup "$BIN" > "$DIR/server.log" 2>&1 &
  for _ in $(seq 1 40); do
    if curl -s -o /dev/null --max-time 1 "http://127.0.0.1:$PORT/"; then
      echo "已启动：http://127.0.0.1:$PORT"
      echo "数据库：$POINTER_ADMIN_DB_URL"
      head -2 "$DIR/server.log"
      return 0
    fi
    sleep 0.25
  done
  echo "启动失败，看 $DIR/server.log"
  tail -5 "$DIR/server.log"
  return 1
}

case "${1:-start}" in
  start) start ;;
  stop) stop; echo "已停止" ;;
  status) status ;;
  reset)
    stop
    rm -rf "$DIR"
    echo "已清空 $DIR，下次 start 是空库，需要重新 seed"
    ;;
  *)
    echo "用法：$0 {start|stop|status|reset}"
    exit 1
    ;;
esac
