#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PID_FILE="$ROOT/.pointer-server.pid"
LOG_FILE="$ROOT/pointer-server.log"

if [[ ! -f "$PID_FILE" ]]; then
  echo "[pointer-server] Status: stopped"
  exit 1
fi

pid="$(tr -d '[:space:]' <"$PID_FILE")"
if [[ -z "$pid" ]] || ! kill -0 "$pid" 2>/dev/null; then
  echo "[pointer-server] Status: stopped (stale PID $pid)"
  rm -f "$PID_FILE"
  exit 1
fi

echo "[pointer-server] Status: running (PID $pid)"
echo "[pointer-server] Log: $LOG_FILE"
