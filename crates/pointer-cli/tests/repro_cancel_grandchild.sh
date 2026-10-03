#!/usr/bin/env bash
# Repro for: pointer-cli cancel/timeout leaves a *grandchild* of the `terminal`
# tool alive.
#
# The terminal tool runs `sh -lc <command>`. A command that forks (e.g. a
# background job, a pipeline, a dev server) puts the long-lived process in a
# grandchild position. The cooperative cancel path used to `kill()` only the
# direct `sh -lc` child, so the grandchild was orphaned and kept running.
#
# The mock provider mode `fork_sleep_tool` runs:
#     sleep 300 & echo $! > "$POINTER_REPRO_PIDFILE"; wait
# so the pid of the forked `sleep 300` grandchild is captured, and this script
# can check whether it outlived the cancelled run.
#
# Usage:
#   bash crates/pointer-cli/tests/repro_cancel_grandchild.sh
#
# Env overrides:
#   POINTER_CLI_BIN   prebuilt binary path (default target/debug/pointer-cli)
#
# Exit code: 0 = no descendant left behind, 1 = residue observed.
set -uo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$HERE/../../.." && pwd)"
BIN="${POINTER_CLI_BIN:-$ROOT/target/debug/pointer-cli}"
export PATH="$HOME/.cargo/bin:$PATH"

WORK="$(mktemp -d)"
PORT=$(( 20000 + RANDOM % 20000 ))
PIDFILE="$WORK/descendant.pid"
MOCK_PID=""

cleanup() {
  if [[ -n "$MOCK_PID" ]]; then
    kill "$MOCK_PID" 2>/dev/null || true
    wait "$MOCK_PID" 2>/dev/null || true
  fi
  # Never leave the repro's own descendant behind if the fix regressed.
  if [[ -s "$PIDFILE" ]]; then
    local pid
    pid="$(tr -dc '0-9' <"$PIDFILE")"
    [[ -n "$pid" ]] && kill -9 "$pid" 2>/dev/null || true
  fi
  rm -rf "$WORK"
}
trap cleanup EXIT

if [[ ! -x "$BIN" ]]; then
  echo "building $BIN ..."
  (cd "$ROOT" && cargo build -p pointer-cli --no-default-features) || exit 1
fi

echo "== starting mock provider (fork_sleep_tool) on port $PORT =="
MOCK_MODE=fork_sleep_tool MOCK_PORT="$PORT" \
  python3 "$HERE/mock_provider.py" >"$WORK/mock.stderr" 2>&1 &
MOCK_PID=$!
for _ in $(seq 1 50); do
  if python3 - "$PORT" <<'PY' 2>/dev/null
import socket, sys
s = socket.socket(); s.settimeout(0.2)
try:
    s.connect(("127.0.0.1", int(sys.argv[1])))
except Exception:
    sys.exit(1)
finally:
    s.close()
PY
  then break; fi
  sleep 0.1
done

echo "== running: pointer-cli run --timeout 5 (terminal tool forks a grandchild) =="
START_MS=$(date +%s%3N)
( cd "$WORK" && POINTER_API_KEY="test-key" POINTER_REPRO_PIDFILE="$PIDFILE" \
    "$BIN" run \
      --workspace "$WORK" \
      --base-url "http://127.0.0.1:$PORT/v1" \
      --model mock-model \
      --instruction "sleep" \
      --timeout 5 ) >"$WORK/stdout.txt" 2>"$WORK/stderr.txt"
RC=$?
END_MS=$(date +%s%3N)
echo "cli exit code: $RC (expected 10 = timeout); elapsed $(( END_MS - START_MS ))ms"

# Give the cancel path a moment to reap, then inspect.
sleep 0.5

if [[ ! -s "$PIDFILE" ]]; then
  echo "FAIL: the terminal command never recorded a descendant pid; repro is inconclusive"
  cat "$WORK/stderr.txt" | tail -5
  exit 1
fi
DESC_PID="$(tr -dc '0-9' <"$PIDFILE")"
echo "grandchild pid recorded by the tool's shell: $DESC_PID"

ALIVE=0
if kill -0 "$DESC_PID" 2>/dev/null && [[ -e "/proc/$DESC_PID" ]]; then
  ALIVE=1
fi

echo "== process table for the descendant =="
ps -o pid,ppid,pgid,stat,args -p "$DESC_PID" 2>/dev/null || echo "(pid $DESC_PID not present)"

if [[ "$ALIVE" -eq 1 ]]; then
  echo "RESIDUE: grandchild $DESC_PID survived the cancelled run"
  exit 1
fi

echo "CLEAN: grandchild $DESC_PID no longer exists after cancel"
exit 0
