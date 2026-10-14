# Start locally

[简体中文](../../zh-CN/deploy/quick-start.md) | English

## Start now

To develop the desktop app, run this from the repository root:

```bash
npm install && ./scripts/quick-deploy.sh start standalone-client
```

```bat
npm install && scripts\quick-deploy.bat start standalone-client
```

Once the window is up, you can change the code. The first compile can take a few minutes. Data lives in `.quick-deploy/standalone-client/data`, separate from an installed Pointer.

To use the same UI in a browser, switch the mode to `standalone-server` and open `http://127.0.0.1:1420/`. The first start writes a local account to `.quick-deploy/standalone-server/standalone-admin.env` and prints the password once.

## Other ways to start

Packaging and rollout stay in [README.md](README.md). The rest of this page is the other cells of the same launcher. It does not read `pointer.local.env`. Each mode sets its own environment, so a managed file on the machine cannot leak into a standalone run. State, logs, and the generated local password live in `.quick-deploy/` (gitignored). Use `./scripts/quick-deploy.sh` on Linux and macOS, and `scripts\quick-deploy.bat` on Windows.

`--runtime` selects how the same four modes are launched:

| `--runtime` | Default | What it launches | Rebuilds? |
| --- | --- | --- | --- |
| `dev` | yes | `cargo run`, the local Vite CLI, or the local Tauri CLI | only when sources, `POINTER_EDITION`, or the three domains changed. Otherwise Cargo reuses the existing debug artifacts |
| `bin` | no | `target/release/pointer-server` or `target/release/pointer-app` | no. A missing binary, or a missing `dist/` for the server, exits immediately |

Dev modes:

| Mode | What it starts | How to tell it worked |
| --- | --- | --- |
| `standalone-server` | `cargo run -p pointer-server` plus the local Vite CLI, with a generated local account | `GET /api/auth/mode` is `standalone`; open `http://127.0.0.1:1420/` |
| `standalone-client` | the local Tauri CLI `dev`, data under `.quick-deploy/standalone-client/data` | Vite listens on `127.0.0.1:1420` and the desktop window opens |
| `managed-server` | The same server and web processes, with `POINTER_EDITION=managed` and `POINTER_DEPLOYMENT_MODE=platform` | `GET /api/auth/mode` is `platform` |
| `managed-client` | `tauri dev` with the three control-plane domains present at compile and run time | The desktop process starts as the managed edition |

The dev server web UI and the dev desktop client both use Vite port `1420`, so only one of them can run. The server API stays on `127.0.0.1:8787`, which is what the Vite `/api` proxy expects. A release server serves `dist/` itself at `http://127.0.0.1:8787/` and does not use port `1420`. The release client does not use port `1420` either. Dev and release servers cannot listen on `8787` at the same time.

## Standalone server

```bash
./scripts/quick-deploy.sh start standalone-server
```

```bat
scripts\quick-deploy.bat start standalone-server
```

The first start writes username `admin`, a random password, and the HMAC to `.quick-deploy/standalone-server/standalone-admin.env`, and prints the password once. Later starts reuse that file and do not print it again.

```bash
./scripts/quick-deploy.sh status
./scripts/quick-deploy.sh stop standalone-server
```

Sign in with the username, password, and captcha. Model keys are still entered in Settings.

## Standalone client

```bash
./scripts/quick-deploy.sh start standalone-client
```

`POINTER_EDITION` is unset and no control plane is bound. The data directory is separate from an installed `PointerApp`.

## Managed server

The three control-plane domains are required. Without a reachable control plane the process still starts in platform mode, but browser login waits on that control plane.

```bash
./scripts/quick-deploy.sh start managed-server \
  --api-base https://pointer-api.example.com \
  --web-base https://pointer.example.com \
  --annotate-base https://pointer-som.example.com
```

```bat
scripts\quick-deploy.bat start managed-server --api-base https://pointer-api.example.com --web-base https://pointer.example.com --annotate-base https://pointer-som.example.com
```

`POINTER_API_BASE`, `POINTER_WEB_BASE`, and `COMPUTER_ANNOTATE_API_BASE` work in place of the flags.

That dev command runs `cargo run -p pointer-server` plus the local Vite CLI on every start. Cargo rebuilds only when it has to: sources changed, or `POINTER_EDITION` and the three domains differ from the last compile (for example, switching over from standalone). When none of those changed, Cargo checks and then runs the existing debug binary. The first switch from standalone to managed usually takes several minutes.

An already-built binary uses `--runtime bin`. That start does not invoke Cargo. The page is at `http://127.0.0.1:8787/`. To bake the domains into the binary, pass `POINTER_EDITION=managed` and the three domains when running `node scripts/build-server.mjs`. A managed build fails if any domain is missing.

```bash
./scripts/quick-deploy.sh start managed-server --runtime bin \
  --api-base https://pointer-api.example.com \
  --web-base https://pointer.example.com \
  --annotate-base https://pointer-som.example.com
```

## Managed client

Same domains. A `POINTER_EDITION=managed` cargo build fails if any domain is missing, because the domains are compiled into the binary.

```bash
./scripts/quick-deploy.sh start managed-client \
  --api-base https://pointer-api.example.com \
  --web-base https://pointer.example.com \
  --annotate-base https://pointer-som.example.com
```

## Compiled binaries

Build the artifact first. The server build is `node scripts/build-server.mjs` (frontend `dist/` and `target/release/pointer-server`). The client build is `node scripts/tauri-build.mjs` (`target/release/pointer-app`). If either file is missing, the script exits and does not compile.

```bash
./scripts/quick-deploy.sh start standalone-server --runtime bin
./scripts/quick-deploy.sh start standalone-client --runtime bin
```

The release standalone server writes its account to `.quick-deploy/bin/standalone-server/standalone-admin.env`, separate from the dev account. Open `http://127.0.0.1:8787/`. `GET /api/health` is `ok`, and `GET /api/auth/mode` is `standalone`.

Managed modes take the same `--runtime bin` flag and the same three domains. The script puts those domains in the process environment and launches the binary that is already on disk. It does not rebuild. A binary produced with `POINTER_EDITION=managed` still needs the three domains at build time.

```bash
./scripts/quick-deploy.sh start managed-server --runtime bin \
  --api-base https://pointer-api.example.com \
  --web-base https://pointer.example.com \
  --annotate-base https://pointer-som.example.com

./scripts/quick-deploy.sh start managed-client --runtime bin \
  --api-base https://pointer-api.example.com \
  --web-base https://pointer.example.com \
  --annotate-base https://pointer-som.example.com
```

`stop standalone-server` stops both the dev and release copies of that mode. To stop only the release copy:

```bash
./scripts/quick-deploy.sh stop standalone-server --runtime bin
```

## Print the plan without starting

```bash
./scripts/quick-deploy.sh plan standalone-server
./scripts/quick-deploy.sh plan standalone-server --runtime bin
./scripts/quick-deploy.sh plan managed-client --api-base https://pointer-api.example.com --web-base https://pointer.example.com --annotate-base https://pointer-som.example.com
```

`stop` with no mode stops every mode and both runtimes recorded under `.quick-deploy/`. It signals the pids in the pid file and does not kill processes by name.
