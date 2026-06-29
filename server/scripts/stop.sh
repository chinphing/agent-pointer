#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PID_FILE="$ROOT/.pointer-server.pid"

if [[ ! -f "$PID_FILE" ]]; then
  echo "[pointer-server] Not running (no PID file)."
  exit 0
fi

pid="$(tr -d '[:space:]' <"$PID_FILE")"
if [[ -z "$pid" ]] || ! kill -0 "$pid" 2>/dev/null; then
  echo "[pointer-server] Stale PID file ($pid); removing."
  rm -f "$PID_FILE"
  exit 0
fi

kill "$pid" 2>/dev/null || true
for _ in $(seq 1 50); do
  kill -0 "$pid" 2>/dev/null || break
  sleep 0.2
done
if kill -0 "$pid" 2>/dev/null; then
  kill -9 "$pid" 2>/dev/null || true
fi

rm -f "$PID_FILE"
echo "[pointer-server] Stopped (PID $pid)."
