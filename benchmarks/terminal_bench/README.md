# terminal-bench adapter for pointer-cli

Harbor adapter that drives the headless [`pointer-cli`](../../crates/pointer-cli)
inside a [Terminal-Bench](https://www.tbench.ai/) task container.

```
benchmarks/terminal_bench/
  pointer_agent.py        # PointerAgent(BaseInstalledAgent) + command builders
  test_pointer_agent.py   # unit tests (stdlib unittest)
  README.md               # this file
```

## What it does

| Phase | Behaviour |
| --- | --- |
| `install()` | Uploads `crates/pointer-cli/dist/pointer-cli-linux-x64.tar.gz` with Harbor's `upload_file` (the container needs **no** `curl`/`wget` and no network), unpacks it, and runs the shipped offline `install.sh --bindir /usr/local/bin`. |
| `run()` | Uploads the instruction as a file (`--instruction-file`, so quotes/newlines/CJK survive), then runs one `pointer-cli run` with `--workspace "$PWD"` (the task workdir) and `--out-dir /logs/agent`. |
| `populate_context_post_run()` | Backfills token counts from `/logs/agent/usage.json`. |

`capabilities = AgentCapabilities(atif=True)` — `pointer-cli` writes the
ATIF v1.7 trajectory to `/logs/agent/trajectory.json`, which Harbor picks up as
`<trial>/agent/trajectory.json`.

### Exit codes

`pointer-cli` exit codes are mapped like this:

| Code | Meaning | Adapter behaviour |
| --- | --- | --- |
| `0` | success | continue |
| `10` | run timeout | **tolerated** — warn, keep the artifacts, let the verifier score |
| `14` | tool-round limit reached | **tolerated** — same |
| `2` / `11` / `12` / `13` | usage / auth / model / internal | raise, classified through Harbor's `ERROR_PATTERNS` (e.g. auth → `AgentAuthenticationError`) |

Tolerating `10`/`14` keeps "agent ran out of budget" a *result* (reward 0)
rather than an infrastructure error.

## Prerequisites

1. **Build the distribution archive** (if it is not already present):

   ```bash
   crates/pointer-cli/packaging/build-dist.sh
   # -> crates/pointer-cli/dist/pointer-cli-linux-x64.tar.gz
   ```

2. **Harbor** available on the same Python that runs the adapter, e.g.:

   ```bash
   python3 -m venv /tmp/harbor-venv && /tmp/harbor-venv/bin/pip install harbor
   ```

## Run

`-a` takes an import path, so `benchmarks/` must be on `PYTHONPATH`:

```bash
cd /path/to/agent-pointer

PYTHONPATH=benchmarks harbor run \
  -a terminal_bench.pointer_agent:PointerAgent \
  -m openai/gpt-4o-mini \
  --ae OPENAI_API_KEY \
  -d terminal-bench/terminal-bench-2-1 \
  -n 4 -k 5
```

A single task (limit the dataset to one trial):

```bash
PYTHONPATH=benchmarks harbor run \
  -a terminal_bench.pointer_agent:PointerAgent \
  -m openai/gpt-4o-mini --ae OPENAI_API_KEY \
  -d terminal-bench/terminal-bench-2-1 -l 1
```

### Dry run (no tasks downloaded, nothing executed)

```bash
PYTHONPATH=benchmarks harbor run --dry-run \
  -a terminal_bench.pointer_agent:PointerAgent \
  -m openai/gpt-4o-mini \
  --ae OPENAI_API_KEY=sk-placeholder \
  -d terminal-bench/terminal-bench-2-1
# -> Dry run OK — 89 trial(s); nothing was run.
```

The dry run validates the import path and the `--ak` kwargs (it does **not**
check that the API key is valid — that happens in `run()`).

## Credentials

`pointer-cli` reads the provider key **only** from the environment variable
named by `--api-key-env`; the adapter always names it `POINTER_API_KEY` and
injects the key Harbor resolved for `-m provider/model`.

```bash
# from the host environment
--ae OPENAI_API_KEY
# explicit value
--ae OPENAI_API_KEY=sk-...
# non-OpenAI provider
--ae DEEPSEEK_API_KEY=... --ak base_url=https://api.deepseek.com
```

`pointer-cli` speaks the **OpenAI-compatible** chat-completions API. The
adapter passes the endpoint Harbor resolves for the selected model, so pick a
model whose provider exposes an OpenAI-compatible `/v1` endpoint, or override
it with `--ak base_url=...`.

## Adapter options (`--ak`)

| Kwarg | Default | Purpose |
| --- | --- | --- |
| `dist_tarball` | `crates/pointer-cli/dist/pointer-cli-linux-x64.tar.gz` | Host path to the archive. Relative paths resolve against the repo root. |
| `workspace` | `$PWD` | Workspace root for pointer-cli's file/terminal tools; `$PWD` expands to the directory Harbor executes in. |
| `approval` | `auto` | Tool approval mode (unattended). |
| `allowed_tools` | `terminal,file_read,file_write,file_edit,file_grep,file_glob,file_list` | Comma-separated tool allow list. |
| `timeout_sec` | `3600` | Wall-clock budget for one `pointer-cli run` (keep it under Harbor's trial timeout). |
| `max_turns` | unset | Optional hard cap on tool-call rounds. |
| `base_url` | Harbor-resolved | Override the OpenAI-compatible endpoint. |

Example with overrides:

```bash
PYTHONPATH=benchmarks harbor run \
  -a terminal_bench.pointer_agent:PointerAgent \
  -m openai/gpt-4o-mini --ae OPENAI_API_KEY \
  --ak timeout_sec=1800 --ak max_turns=100 \
  -d terminal-bench/terminal-bench-2-1
```

## Tests

The tests need the interpreter that has `harbor` installed; no extra test
dependency (stdlib `unittest`):

```bash
PYTHONPATH=benchmarks /tmp/harbor-venv/bin/python -m unittest terminal_bench.test_pointer_agent -v
# Ran 30 tests ... OK
```

They cover the install command, the run command, model/credential resolution,
exit-code handling, and `usage.json` parsing — all against a fake environment,
so no container and no network are involved.

## Known limitations

- **Linux x64 only.** The published archive is a statically linked
  `x86_64-unknown-linux-musl` binary; Windows/macOS task containers are not
  supported (declare `capabilities.windows = False`, the default).
- **No `resume` / `load_trajectory`.** `pointer-cli` has no session-resume
  entry point yet, so multi-step trials start fresh each step.
- **Trajectory format is owned by pointer-cli**, not this adapter; the adapter
  only points `--out-dir` at `/logs/agent`.
