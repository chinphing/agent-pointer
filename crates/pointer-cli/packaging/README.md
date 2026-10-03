# pointer-cli (linux-x64)

Headless, GUI-free runner for the Pointer agent runtime. One instruction goes
in, artifacts and a semantic exit code come out — no display server, no Tauri,
no axum.

This archive contains a **statically linked** `x86_64-unknown-linux-musl`
binary. It has **no shared-library dependencies**: it runs on glibc and musl
distributions alike (Debian/Ubuntu, Alpine, distroless, …), and needs no
`libc`, no X11/Wayland, no GBM, no `libsqlite3`.

## Contents

```
pointer-cli-linux-x64/
  pointer-cli   statically linked binary (stripped)
  install.sh      offline installer
  README.md       this file
  LICENSE         Apache-2.0
  SHA256SUMS      checksum for ./pointer-cli
```

## Install

No network access is required.

```sh
tar -xzf pointer-cli-linux-x64.tar.gz
cd pointer-cli-linux-x64
sudo ./install.sh                 # -> /usr/local/bin/pointer-cli
```

Without root, or to a custom location:

```sh
./install.sh --prefix "$HOME/.local"      # -> ~/.local/bin/pointer-cli
./install.sh --bindir /opt/bin            # -> /opt/bin/pointer-cli
```

`install.sh` verifies `SHA256SUMS` before copying (skipped with a warning when
neither `sha256sum` nor `shasum` exists) and prints the installed version.

The archive itself is checksummed separately:

```sh
sha256sum -c pointer-cli-linux-x64.tar.gz.sha256
```

## Usage

```sh
pointer-cli --version
pointer-cli --help

pointer-cli run \
  --instruction "List the files in the workspace and summarize them." \
  --workspace /workspace \
  --model gpt-4o-mini \
  --base-url https://api.openai.com/v1 \
  --api-key-env POINTER_API_KEY \
  --out-dir /logs/agent \
  --timeout 600
```

`--instruction` also accepts `--instruction-file PATH`, which is convenient for
long, quoting-sensitive prompts.

### Credentials

The API key is read **only** from the environment variable named by
`--api-key-env` (default `POINTER_API_KEY`). The CLI never reads the desktop
app's encrypted settings, and it never touches `~/.local/share/PointerApp`
unless you pass `--data-dir`; otherwise it uses a throwaway temp directory.

```sh
POINTER_API_KEY=sk-... pointer-cli run --instruction "..."
```

### Useful switches

| Flag | Purpose |
| --- | --- |
| `--workspace DIR` | Workspace root for the file / terminal tools (default `.`) |
| `--allowed-tools a,b` | Restrict the tool set (`file`, `terminal`, … or exact ids) |
| `--max-turns N` | Hard cap on tool-call rounds |
| `--timeout SECS` | Wall-clock deadline; on expiry the run is cancelled and the child process is reaped |
| `--out-dir DIR` | Where `result.json` + observability artifacts are written |
| `--events jsonl` | Mirror the event stream as JSONL on stdout (stdout then carries events only) |
| `--approval auto` | Tool approval mode; `auto` is the unattended default |
| `--dry-run` | Resolve configuration and exit without calling the model |

### Artifacts

With `--out-dir DIR` the CLI writes:

- `result.json` — final reply, outcome, usage
- `events.jsonl` — full event stream
- `agent.log` — runtime log
- `usage.json` — token usage
- `trajectory.json` — ATIF v1.7 trajectory

### Exit codes

| Code | Meaning |
| --- | --- |
| `0` | Success |
| `2` | Usage error |
| `10` | Timeout |
| `11` | Authentication / authorization failure |
| `12` | Model or provider failure |
| `13` | Internal error |
| `14` | Tool-round limit reached |

## Running in a container

The binary needs no base image libraries, so any image works:

```sh
docker run --rm -v "$PWD:/workspace" \
  -e POINTER_API_KEY \
  pointer-cli:dev run --instruction "..." --workspace /workspace
```

A ready-made smoke image lives at `Dockerfile.agent` in the repository root:

```sh
crates/pointer-cli/packaging/build-dist.sh
docker build -f Dockerfile.agent -t pointer-cli:dev crates/pointer-cli/dist
docker run --rm pointer-cli:dev --version
```

## Notes

- PDF rendering loads `libpdfium` lazily from the pdfium-auto cache
  (`~/.cache/pdf2md/pdfium-*`), downloading it on first use when absent. This is
  unrelated to the binary itself and never affects `--version` or text/file/
  terminal tool runs.
- No `skills/` directory ships with this archive. The CLI runs with an empty
  skill set; skills are optional user-supplied runtime data.
- The binary is stripped, so Rust backtraces contain addresses only. Use a debug
  build when you need symbol names.
