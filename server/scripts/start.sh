#!/usr/bin/env bash
# Start pointer-server in the background (same directory as this script).
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BIN="$ROOT/pointer-server"
PID_FILE="$ROOT/.pointer-server.pid"
LOG_FILE="$ROOT/pointer-server.log"

if [[ ! -f "$BIN" ]]; then
  echo "[pointer-server] Binary not found: $BIN" >&2
  exit 1
fi

if [[ -f "$PID_FILE" ]]; then
  pid="$(tr -d '[:space:]' <"$PID_FILE")"
  if [[ -n "$pid" ]] && kill -0 "$pid" 2>/dev/null; then
    echo "[pointer-server] Already running (PID $pid)."
    exit 0
  fi
  rm -f "$PID_FILE"
fi

cd "$ROOT"
nohup "$BIN" >>"$LOG_FILE" 2>&1 &
echo "$!" >"$PID_FILE"
echo "[pointer-server] Started (PID $(cat "$PID_FILE"))."
echo "[pointer-server] Log: $LOG_FILE"
