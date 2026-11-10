#!/bin/sh
# Pointer 控制面启停脚本（生产用，不依赖 node / npm / 仓库）
#
#   ./pointer-admin.sh start     后台启动，写 PID 文件
#   ./pointer-admin.sh stop      优雅停止（TERM，10 秒后 KILL）
#   ./pointer-admin.sh restart
#   ./pointer-admin.sh status    运行中退出 0，已停止退出 1
#
# 放在二进制同一目录下即可，不需要任何参数。
#
# 环境变量：
#   POINTER_ADMIN_STATIC_DIR   默认 ./web；指向不存在的位置会拒绝启动
#   其余（DB_URL / DATA_DIR / OAUTH_SECRET / ADDR …）原样透传，脚本不猜
#
# 退出码：0 成功；1 失败。status 特殊：运行中 0，已停止 1。

set -u

BASE_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
BIN="$BASE_DIR/pointer-admin"
PID_FILE="$BASE_DIR/pointer-admin.pid"
LOG_FILE="$BASE_DIR/pointer-admin.log"

log() { echo "[pointer-admin] $*"; }
fail() { echo "[pointer-admin] $*" >&2; exit 1; }

read_pid() {
  [ -f "$PID_FILE" ] || return 1
  _pid=$(cat "$PID_FILE" 2>/dev/null || true)
  case "$_pid" in
    '' | *[!0-9]*) return 1 ;;
  esac
  echo "$_pid"
}

is_alive() {
  [ -n "${1:-}" ] || return 1
  kill -0 "$1" 2>/dev/null
}

do_start() {
  [ -f "$BIN" ] || fail "找不到二进制：$BIN"
  [ -x "$BIN" ] || chmod +x "$BIN" 2>/dev/null || true
  [ -x "$BIN" ] || fail "二进制不可执行：$BIN"

  _static="${POINTER_ADMIN_STATIC_DIR:-$BASE_DIR/web}"
  [ -f "$_static/index.html" ] ||
    fail "找不到前端产物：$_static/index.html —— 控制面起来会白屏，拒绝启动"

  _pid=$(read_pid || true)
  if is_alive "${_pid:-}"; then
    log "已在运行（PID ${_pid}）"
    return 0
  fi
  [ -n "${_pid:-}" ] && rm -f "$PID_FILE"

  cd "$BASE_DIR" || fail "无法进入 $BASE_DIR"

  POINTER_ADMIN_STATIC_DIR="$_static" nohup "$BIN" >>"$LOG_FILE" 2>&1 &
  _new=$!
  echo "$_new" >"$PID_FILE"

  # 给进程一点时间暴露启动失败（端口占用、数据库打不开都在头一秒）
  _i=0
  while [ "$_i" -lt 10 ]; do
    is_alive "$_new" || break
    sleep 0.2
    _i=$((_i + 1))
  done

  if ! is_alive "$_new"; then
    log "启动后立即退出，最后 20 行日志："
    tail -n 20 "$LOG_FILE" 2>/dev/null || true
    rm -f "$PID_FILE"
    return 1
  fi

  log "已启动（PID ${_new}）"
  log "静态资源：$_static"
  log "日志：$LOG_FILE"
}

do_stop() {
  _pid=$(read_pid || true)
  if [ -z "${_pid:-}" ]; then
    log "未在运行（没有 PID 文件）"
    return 0
  fi
  if ! is_alive "$_pid"; then
    log "PID 文件已过期（${_pid}），清理"
    rm -f "$PID_FILE"
    return 0
  fi

  kill -TERM "$_pid" 2>/dev/null || true
  _i=0
  while [ "$_i" -lt 50 ]; do
    is_alive "$_pid" || break
    sleep 0.2
    _i=$((_i + 1))
  done
  if is_alive "$_pid"; then
    log "10 秒内未退出，强制结束"
    kill -KILL "$_pid" 2>/dev/null || true
  fi

  rm -f "$PID_FILE"
  log "已停止（PID ${_pid}）"
}

do_status() {
  _pid=$(read_pid || true)
  if [ -z "${_pid:-}" ]; then
    log "状态：已停止"
    return 1
  fi
  if ! is_alive "$_pid"; then
    log "状态：已停止（PID $_pid 已过期）"
    rm -f "$PID_FILE"
    return 1
  fi
  log "状态：运行中（PID ${_pid}）"
  log "日志：$LOG_FILE"
  return 0
}

case "${1:-status}" in
  start) do_start ;;
  stop) do_stop ;;
  restart) do_stop && do_start ;;
  status) do_status ;;
  *)
    echo "用法：$0 {start|stop|restart|status}" >&2
    exit 1
    ;;
esac
