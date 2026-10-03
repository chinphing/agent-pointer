#!/usr/bin/env bash
# End-to-end controllability checks for pointer-cli against a local mock
# OpenAI-compatible provider (no network, no real credentials).
#
# Usage:
#   bash crates/pointer-cli/tests/e2e_controls.sh
#
# Env overrides:
#   POINTER_CLI_BIN   prebuilt binary path (default target/debug/pointer-cli)
set -uo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$HERE/../../.." && pwd)"
BIN="${POINTER_CLI_BIN:-$ROOT/target/debug/pointer-cli}"
export PATH="$HOME/.cargo/bin:$PATH"

WORK="$(mktemp -d)"
PORT=$(( 20000 + RANDOM % 20000 ))
MOCK_LOG="$WORK/requests.jsonl"
MOCK_PID=""
PASS=0
FAIL=0

cleanup() {
  stop_mock
  rm -rf "$WORK"
}
trap cleanup EXIT

log() { printf '%s\n' "$*"; }
ok()  { PASS=$((PASS + 1)); log "PASS: $*"; }
bad() { FAIL=$((FAIL + 1)); log "FAIL: $*"; }

if [[ ! -x "$BIN" ]]; then
  log "building $BIN ..."
  (cd "$ROOT" && cargo build -p pointer-cli --no-default-features) || exit 1
fi

start_mock() {
  local mode="$1"
  : > "$MOCK_LOG"
  MOCK_MODE="$mode" MOCK_LOG="$MOCK_LOG" MOCK_PORT="$PORT" \
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
    then return 0; fi
    sleep 0.1
  done
  log "mock provider failed to start"; cat "$WORK/mock.stderr"; exit 1
}

stop_mock() {
  if [[ -n "$MOCK_PID" ]]; then
    kill "$MOCK_PID" 2>/dev/null || true
    wait "$MOCK_PID" 2>/dev/null || true
    MOCK_PID=""
  fi
}

# run_agent <outdir> [args...]  -> sets RC
run_agent() {
  local out="$1"; shift
  mkdir -p "$out"
  ( cd "$WORK" && POINTER_API_KEY="test-key" "$BIN" run \
      --workspace "$WORK" \
      --base-url "http://127.0.0.1:$PORT/v1" \
      --model mock-model \
      --out-dir "$out" "$@" ) >"$out/stdout.txt" 2>"$out/stderr.txt"
  RC=$?
}

result_field() { # result_field <outdir> <json-path-expr>
  python3 - "$1/result.json" "$2" <<'PY'
import json, sys
try:
    doc = json.load(open(sys.argv[1], encoding="utf-8"))
except Exception:
    print(""); raise SystemExit(0)
cur = doc
for part in sys.argv[2].split("."):
    if part == "":
        continue
    if isinstance(cur, list):
        cur = cur[int(part)]
    else:
        cur = cur.get(part)
    if cur is None:
        break
print("" if cur is None else cur)
PY
}

mock_tools() { # mock_tools -> comma separated sorted tools advertised on request 1
  python3 - "$MOCK_LOG" <<'PY'
import json, sys
try:
    first = json.loads(open(sys.argv[1], encoding="utf-8").readline())
except Exception:
    print(""); raise SystemExit(0)
print(",".join(sorted(first.get("tools") or [])))
PY
}

log "== scenario A: usage error (no instruction) =="
( cd "$WORK" && POINTER_API_KEY="test-key" "$BIN" run --workspace "$WORK" \
    --out-dir "$WORK/outA" ) >"$WORK/outA.txt" 2>&1
RC=$?
[[ $RC -eq 2 ]] && ok "missing instruction -> exit 2" || bad "missing instruction -> expected 2, got $RC"

log "== scenario B: missing API key -> auth exit =="
( cd "$WORK" && env -u POINTER_API_KEY "$BIN" run --instruction "hi" \
    --workspace "$WORK" --out-dir "$WORK/outB" ) >"$WORK/outB.txt" 2>&1
RC=$?
[[ $RC -eq 11 ]] && ok "missing api key -> exit 11" || bad "missing api key -> expected 11, got $RC"

log "== scenario C: tool whitelist filters the model-visible tool list =="
start_mock text_only
run_agent "$WORK/outC" --instruction "say hi" --allowed-tools terminal,file_read
[[ $RC -eq 0 ]] && ok "whitelist run -> exit 0" || bad "whitelist run -> expected 0, got $RC"
TOOLS="$(mock_tools)"
[[ "$TOOLS" == "file_read,terminal" ]] \
  && ok "provider saw exactly the allow list tools ($TOOLS)" \
  || bad "provider tools expected 'file_read,terminal', got '$TOOLS'"
KEPT="$(result_field "$WORK/outC" allowed_tools.kept)"
[[ "$KEPT" == "['file_read', 'terminal']" ]] \
  && ok "result.json allowed_tools.kept = $KEPT" \
  || bad "result.json allowed_tools.kept unexpected: $KEPT"
REPLY="$(result_field "$WORK/outC" assistant_reply)"
[[ "$REPLY" == "MOCK_FINAL_OK" ]] && ok "assistant reply captured" || bad "assistant reply: '$REPLY'"
stop_mock

log "== scenario D: tool-round cap -> exit 14 =="
start_mock tool_forever
run_agent "$WORK/outD" --instruction "loop" --max-turns 2 --timeout 60
[[ $RC -eq 14 ]] && ok "max-turns 2 -> exit 14" || bad "max-turns 2 -> expected 14, got $RC"
USED="$(result_field "$WORK/outD" tool_rounds_used)"
MAX="$(result_field "$WORK/outD" max_tool_rounds)"
STATUS="$(result_field "$WORK/outD" status)"
[[ "$USED" == "2" && "$MAX" == "2" && "$STATUS" == "turn_limit" ]] \
  && ok "result.json reports turn_limit used=$USED max=$MAX" \
  || bad "result.json turn-limit fields: status=$STATUS used=$USED max=$MAX"
stop_mock

log "== scenario E: timeout interrupts an in-flight terminal tool call -> exit 10 =="
# Baseline: the terminal tool's own default timeout is 30s. A 5s --timeout that
# returns in ~5s proves the cancel (not the tool timeout) ended the run.
start_mock sleep_tool
START_MS=$(date +%s%3N)
run_agent "$WORK/outE" --instruction "sleep" --timeout 5
END_MS=$(date +%s%3N)
ELAPSED_MS=$(( END_MS - START_MS ))
[[ $RC -eq 10 ]] && ok "timeout -> exit 10" || bad "timeout -> expected 10, got $RC"
[[ $ELAPSED_MS -lt 15000 ]] \
  && ok "cancelled the tool call promptly (${ELAPSED_MS}ms; tool default is 30s)" \
  || bad "timeout took ${ELAPSED_MS}ms (expected < 15000)"
STATUS="$(result_field "$WORK/outE" status)"
[[ "$STATUS" == "timeout" ]] && ok "result.json status=timeout" || bad "result.json status=$STATUS"
sleep 0.5
# NOTE: match the *tool's* shell precisely (`sh -lc sleep 300`), never a bare
# `sh -lc` — the host running this script is itself a `sh -lc …` process.
if pgrep -f "sh -lc sleep 300" >/dev/null 2>&1; then
  bad "the terminal tool's shell process is still alive after cancel"
else
  ok "the terminal tool's shell process was reaped by cancel"
fi
# `sh -lc "sleep 300"` forks, so the long-lived `sleep` is a *grandchild* of the
# CLI. The cancel path now SIGKILLs the tool's whole process group, so both the
# shell above and that grandchild are reaped; `pkill` remains as a defensive
# cleanup only.
pkill -f "sleep 300" 2>/dev/null || true
stop_mock

log "== scenario E2: cancel reaps the tool's own process (exec form) =="
start_mock exec_sleep_tool
run_agent "$WORK/outE2" --instruction "sleep" --timeout 5
[[ $RC -eq 10 ]] && ok "timeout (exec form) -> exit 10" || bad "timeout (exec form) -> expected 10, got $RC"
sleep 0.5
if pgrep -f "sleep 300" >/dev/null 2>&1; then
  bad "the 'sleep 300' process is still alive after cancel"
  pkill -f "sleep 300" 2>/dev/null || true
else
  ok "the tool's long-running child ('sleep 300') was killed by cancel"
fi
stop_mock

log "== scenario F: default invocation unchanged (no new flags) =="
start_mock text_only
run_agent "$WORK/outF" --instruction "say hi"
[[ $RC -eq 0 ]] && ok "default run -> exit 0" || bad "default run -> expected 0, got $RC"
TOOLS="$(mock_tools)"
[[ "$TOOLS" == *"web_search"* && "$TOOLS" == *"terminal"* ]] \
  && ok "default run exposes the full registry ($TOOLS)" \
  || bad "default run tool list looks filtered: $TOOLS"
stop_mock

log "== scenario G: reasoning-only finish_reason=length -> escalate budget, do not fail =="
# Regression: a thinking model that spent its whole output budget on reasoning
# (no visible content, finish_reason=length) must not be retried identically to
# a fatal "output truncated" error. The retry must raise max_tokens and recover.
start_mock reasoning_length_then_text
run_agent "$WORK/outG" --instruction "think hard"
[[ $RC -eq 0 ]] && ok "reasoning-only length run -> exit 0" || bad "reasoning-only length run -> expected 0, got $RC"
REPLY="$(result_field "$WORK/outG" assistant_reply)"
[[ "$REPLY" == "MOCK_FINAL_OK" ]] \
  && ok "run recovered to a visible reply after the reasoning-only stop" \
  || bad "assistant reply after recovery: '$REPLY'"
BUDGETS="$(python3 - "$MOCK_LOG" <<'PY'
import json, sys
rows = [json.loads(l) for l in open(sys.argv[1], encoding="utf-8") if l.strip()]
print(",".join("" if r.get("max_tokens") is None else str(r.get("max_tokens")) for r in rows))
PY
)"
FIRST="${BUDGETS%%,*}"; SECOND="${BUDGETS##*,}"
if [[ -n "$FIRST" && -n "$SECOND" && "$SECOND" -gt "$FIRST" && "$SECOND" -ge 16384 ]]; then
  ok "retry escalated max_tokens ($BUDGETS)"
else
  bad "expected escalated budgets (second > first, second >= 16384), got '$BUDGETS'"
fi
stop_mock

log "== scenario H: ATIF trajectory embeds sub-agent steps =="
# A delegated sub-agent is a scoped transcript (anchor_message_id set) in the
# conversation store; the ATIF builder must surface it under
# `subagent_trajectories` (and reference it from the spawning step) without
# turning its rows into lead steps.
start_mock subagent_then_text
run_agent "$WORK/outH" --instruction "delegate a subtask"
[[ $RC -eq 0 ]] && ok "sub-agent run -> exit 0" || bad "sub-agent run -> expected 0, got $RC"
python3 - "$WORK/outH/trajectory.json" <<'PY'
import json, sys

try:
    doc = json.load(open(sys.argv[1], encoding="utf-8"))
except Exception as exc:  # noqa: BLE001 - surface any read/parse failure
    print(f"trajectory.json unreadable: {exc}")
    raise SystemExit(1)

problems = []
if doc.get("schema_version") != "ATIF-v1.7":
    problems.append(f"schema_version={doc.get('schema_version')!r}")
lead_steps = doc.get("steps") or []
if len(lead_steps) != 3:
    problems.append(f"lead steps={len(lead_steps)} (expected 3: user + tool call + reply)")

subs = doc.get("subagent_trajectories") or []
if not subs:
    problems.append("subagent_trajectories is empty")

for i, sub in enumerate(subs):
    if not sub.get("trajectory_id"):
        problems.append(f"sub[{i}] missing trajectory_id")
    steps = sub.get("steps") or []
    if not steps:
        problems.append(f"sub[{i}] has no steps")
        continue
    ids = [step.get("step_id") for step in steps]
    if ids != list(range(1, len(steps) + 1)):
        problems.append(f"sub[{i}] step_ids={ids} not sequential from 1")
    if not any(step.get("source") == "agent" for step in steps):
        problems.append(f"sub[{i}] has no agent step")

refs = []
for step in lead_steps:
    for result in (step.get("observation") or {}).get("results") or []:
        for ref in result.get("subagent_trajectory_ref") or []:
            refs.append(ref.get("trajectory_id"))
if sorted(r for r in refs if r) != sorted(s.get("trajectory_id") for s in subs):
    problems.append(
        f"lead refs {refs} do not match embedded "
        f"{[s.get('trajectory_id') for s in subs]}"
    )

if problems:
    print("; ".join(problems))
    raise SystemExit(1)
print("ok")
PY
[[ $? -eq 0 ]] \
  && ok "trajectory.json embeds visible sub-agent steps and references them" \
  || bad "trajectory.json sub-agent check failed"
stop_mock

log ""
log "== summary: $PASS passed, $FAIL failed =="
[[ $FAIL -eq 0 ]]
