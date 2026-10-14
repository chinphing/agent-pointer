# DeepSWE adapter for pointer-cli

[Pier](https://github.com/datacurve-ai/pier) agent adapter that drives the
headless [`pointer-cli`](../../crates/pointer-cli) against the
[DeepSWE](https://deepswe.datacurve.ai/) task corpus.

```
benchmarks/deepswe/
  pointer_agent.py        # PointerAgent(BaseInstalledAgent) + pure command builders
  test_pointer_agent.py   # unit tests (stdlib unittest)
  __init__.py             # re-exports PointerAgent
  README.md               # this file
```

This is the Pier counterpart of `benchmarks/terminal_bench/pointer_agent.py`
(Harbor). The two live side by side; neither imports the other.

## What it does

| Phase | Behaviour |
| --- | --- |
| `install_spec()` | Returns **`None`**. DeepSWE ships a prebuilt Docker image per task; declaring an install spec would make Pier rebuild that image with an agent layer. Returning `None` keeps the image untouched. |
| `install()` | Uploads `crates/pointer-cli/dist/pointer-cli-linux-x64.tar.gz` with `environment.upload_file` (the container needs **no** `curl`/`wget` and no network), unpacks it as root, and runs the shipped offline `install.sh --bindir /usr/local/bin`. |
| `run()` | Uploads the instruction as a file (`--instruction-file`, so quotes/newlines/CJK survive), then runs one `pointer-cli run` via `exec_as_agent`, so Pier's `agent_process_env` proxy variables reach the process. Finally a **best-effort finalize commit** stages and commits any leftover work (see below). |
| `network_allowlist()` | Derives the inference host from `base_url` / `POINTER_BASE_URL` / `OPENAI_BASE_URL` (default `api.openai.com`) so Pier's filtered-egress proxy admits the model call. |
| `populate_context_post_run()` | Backfills token counts from `/logs/agent/usage.json`. |

`SUPPORTS_ATIF = True` — `pointer-cli` writes the ATIF v1.7 trajectory to
`/logs/agent/trajectory.json`, which Pier picks up as
`<trial>/agent/trajectory.json`.

### Why `exec_as_agent` matters

Pier only injects the filtered-egress proxy (`HTTP_PROXY` / `HTTPS_PROXY`) in
`BaseInstalledAgent._exec()`, which backs `exec_as_agent` / `exec_as_root`.
Calling `environment.exec()` directly would bypass that injection and the
`no-network` DeepSWE task would fail on the first model call. `run()` therefore
always goes through `exec_as_agent`. `pointer-cli` uses `reqwest`, which honours
the proxy environment variables by default.

### Exit codes

`pointer-cli` exit codes are mapped like this:

| Code | Meaning | Adapter behaviour |
| --- | --- | --- |
| `0` | success | continue |
| `10` | run timeout | **tolerated** — the wrapper maps it to `0`; the verifier scores the artifacts |
| `14` | tool-round limit reached | **tolerated** — same |
| `2` / `11` / `12` / `13` | usage / auth / model / internal | propagate, so Pier raises `NonZeroAgentExitCodeError` |

Tolerating `10`/`14` keeps "agent ran out of budget" a *result* (reward 0)
rather than an infrastructure error.

## Prerequisites

1. **Build the distribution archive** (if it is not already present):

   ```bash
   crates/pointer-cli/packaging/build-dist.sh
   # -> crates/pointer-cli/dist/pointer-cli-linux-x64.tar.gz
   ```

2. **Pier** available on the Python that runs the adapter:

   ```bash
   uv tool install datacurve-pier     # or: pip install datacurve-pier
   ```

## Run

`--agent-import-path` takes `module.path:ClassName`, so `benchmarks/` must be on
`PYTHONPATH`:

```bash
cd /path/to/agent-pointer

PYTHONPATH=benchmarks pier run \
  -p /path/to/deep-swe/tasks \
  --agent-import-path deepswe.pointer_agent:PointerAgent \
  -m openai/gpt-5.5
```

A single task and a deterministic subset:

```bash
PYTHONPATH=benchmarks pier run \
  -p /path/to/deep-swe/tasks/abs-module-cache-flags \
  --agent-import-path deepswe.pointer_agent:PointerAgent \
  -m openai/gpt-5.5

PYTHONPATH=benchmarks pier run \
  -p /path/to/deep-swe/tasks \
  --agent-import-path deepswe.pointer_agent:PointerAgent \
  -m openai/gpt-5.5 --n-tasks 10 --sample-seed 0
```

## Credentials

`pointer-cli` reads the provider key **only** from the environment variable
named by `--api-key-env`; the adapter always names it `POINTER_API_KEY` and
injects the key it resolves from the host environment (or `agent.env`):

```bash
# host environment (auto-detected, in order)
POINTER_API_KEY / OPENAI_API_KEY / ANTHROPIC_API_KEY

# explicit value via --ae (agent.env)
--ae POINTER_API_KEY=sk-...

# forward a host variable through Pier's ${VAR} templating
--ae POINTER_API_KEY='${OPENAI_API_KEY}'
```

`pointer-cli` speaks the **OpenAI-compatible** chat-completions API. Pick a
model whose provider exposes an OpenAI-compatible `/v1` endpoint, or override
the endpoint with `--ak base_url=...`.

## Adapter options (`--ak`)

| Kwarg | Default | Purpose |
| --- | --- | --- |
| `dist_tarball` | `crates/pointer-cli/dist/pointer-cli-linux-x64.tar.gz` | Host path to the archive. Relative paths resolve against the repo root. |
| `workspace` | `$PWD` | Workspace root for pointer-cli's file/terminal tools; `$PWD` expands to the directory Pier executes in (the task workdir). |
| `approval` | `auto` | Tool approval mode (unattended). |
| `allowed_tools` | `terminal,file_read,file_write,file_edit,file_grep,file_glob,file_list` | Comma-separated tool allow list. |
| `timeout_sec` | `3600` | Wall-clock budget for one `pointer-cli run`. Keep it under the task's `[agent] timeout_sec`. |
| `max_turns` | unset | Optional hard cap on tool-call rounds. |
| `base_url` | env / provider default | Override the OpenAI-compatible endpoint. Also feeds `network_allowlist()`. |

Example with overrides:

```bash
PYTHONPATH=benchmarks pier run \
  -p /path/to/deep-swe/tasks \
  --agent-import-path deepswe.pointer_agent:PointerAgent \
  -m deepseek/deepseek-chat \
  --ae DEEPSEEK_API_KEY='${DEEPSEEK_API_KEY}' \
  --ak base_url=https://api.deepseek.com/v1 \
  --ak timeout_sec=1800 --ak max_turns=100
```

## Tests

The tests need the interpreter that has `datacurve-pier` installed; no extra
test dependency (stdlib `unittest`):

```bash
PYTHONPATH=benchmarks /tmp/pier-venv/bin/python -m unittest deepswe.test_pointer_agent -v
# Ran 41 tests ... OK
```

They cover the install command, the run command, the exit-code wrapper,
model/credential/base-url resolution, the network allowlist, `install_spec()`
being `None`, and `usage.json` parsing — all against a fake environment, so no
container and no network are involved.

### Static parse check

Confirms Pier can resolve the import path and read the agent's declarative
surface without running a task:

```bash
PYTHONPATH=benchmarks /tmp/pier-venv/bin/python - <<'PY'
from pathlib import Path
from pier.agents.factory import AgentFactory

agent = AgentFactory.create_agent_from_import_path(
    "deepswe.pointer_agent:PointerAgent",
    logs_dir=Path("/tmp/deepswe-agent-logs"),
    model_name="openai/gpt-5.5",
)
print(agent.name(), agent.install_spec(), agent.network_allowlist().domains)
PY
```

## Known limitations

- **Linux x64 only.** The published archive is a statically linked
  `x86_64-unknown-linux-musl` binary; Windows task containers are not supported
  (`SUPPORTS_WINDOWS` stays `False`).
- **No `resume` / `load_trajectory`.** `pointer-cli` has no session-resume entry
  point yet, so multi-step trials start fresh each step.
- **Trajectory format is owned by pointer-cli**, not this adapter; the adapter
  only points `--out-dir` at `/logs/agent`.
- **Cost is not reported.** `pointer-cli` writes `cost_usd: null`, so
  `AgentContext.cost_usd` stays unset.
