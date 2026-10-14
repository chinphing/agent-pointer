# Local quick start for the four deployment cells

[简体中文](../../zh-CN/deploy/quick-start.md) | English

Packaging and rollout stay in [README.md](README.md). This page starts the same kind of local smoke processes used to check that one deployment cell actually comes up.

The entry point is `scripts/quick-deploy.mjs` at the repo root (`npm run quick-deploy -- …`). It does not read `pointer.local.env`. Each mode sets its own environment, so a managed file on the machine cannot leak into a standalone run. State, logs, and the generated local password live in `.quick-deploy/` (gitignored).

`--runtime` selects how the same four modes are launched:

| `--runtime` | Default | What it launches | Compiles? |
| --- | --- | --- | --- |
| `dev` | yes | `cargo run`, Vite, or `tauri dev` | yes; the first start can take several minutes |
| `bin` | no | `target/release/pointer-server` or `target/release/pointer-app` | no; a missing binary exits immediately |

Dev modes:

| Mode | What it starts | How to tell it worked |
| --- | --- | --- |
| `standalone-server` | `cargo run -p pointer-server` plus `npm run web:dev`, with a generated local account | `GET /api/auth/mode` is `standalone`; open `http://127.0.0.1:1420/` |
| `standalone-client` | `npx tauri dev`, data under `.quick-deploy/standalone-client/data` | Vite listens on `127.0.0.1:1420` and the desktop window opens |
| `managed-server` | The same server and web processes, with `POINTER_EDITION=managed` and `POINTER_DEPLOYMENT_MODE=platform` | `GET /api/auth/mode` is `platform` |
| `managed-client` | `tauri dev` with the three control-plane domains present at compile and run time | The desktop process starts as the managed edition |

The dev server web UI and the dev desktop client both use Vite port `1420`, so only one of them can run. The server API stays on `127.0.0.1:8787`, which is what the Vite `/api` proxy expects. A release server serves `dist/` itself at `http://127.0.0.1:8787/` and does not use port `1420`. The release client does not use port `1420` either. Dev and release servers cannot listen on `8787` at the same time.

## Standalone server

```bash
npm run quick-deploy -- start standalone-server
```

The first start writes username `admin`, a random password, and the HMAC to `.quick-deploy/standalone-server/standalone-admin.env`, and prints the password once. Later starts reuse that file and do not print it again.

```bash
npm run quick-deploy -- status
npm run quick-deploy -- stop standalone-server
```

Sign in with the username, password, and captcha. Model keys are still entered in Settings.

## Standalone client

```bash
npm run quick-deploy -- start standalone-client
```

`POINTER_EDITION` is unset and no control plane is bound. The data directory is separate from an installed `PointerApp`.

## Managed server

The three control-plane domains are required. Without a reachable control plane the process still starts in platform mode, but browser login waits on that control plane.

```bash
npm run quick-deploy -- start managed-server \
  --api-base https://pointer-api.example.com \
  --web-base https://pointer.example.com \
  --annotate-base https://pointer-som.example.com
```

`POINTER_API_BASE`, `POINTER_WEB_BASE`, and `COMPUTER_ANNOTATE_API_BASE` work in place of the flags.

## Managed client

Same domains. A `POINTER_EDITION=managed` cargo build fails if any domain is missing, because the domains are compiled into the binary.

```bash
npm run quick-deploy -- start managed-client \
  --api-base https://pointer-api.example.com \
  --web-base https://pointer.example.com \
  --annotate-base https://pointer-som.example.com
```

## Compiled binaries

Build the artifact first. The server build is `npm run server:build` (frontend `dist/` and `target/release/pointer-server`). The client build is `npm run tauri:build` (`target/release/pointer-app`). If either file is missing, the script exits and does not compile.

```bash
npm run quick-deploy -- start standalone-server --runtime bin
npm run quick-deploy -- start standalone-client --runtime bin
```

The release standalone server writes its account to `.quick-deploy/bin/standalone-server/standalone-admin.env`, separate from the dev account. Open `http://127.0.0.1:8787/`. `GET /api/health` is `ok`, and `GET /api/auth/mode` is `standalone`.

Managed modes take the same `--runtime bin` flag and the same three domains. The script puts those domains in the process environment and launches the binary that is already on disk. It does not rebuild. A binary produced with `POINTER_EDITION=managed` still needs the three domains at build time.

```bash
npm run quick-deploy -- start managed-server --runtime bin \
  --api-base https://pointer-api.example.com \
  --web-base https://pointer.example.com \
  --annotate-base https://pointer-som.example.com

npm run quick-deploy -- start managed-client --runtime bin \
  --api-base https://pointer-api.example.com \
  --web-base https://pointer.example.com \
  --annotate-base https://pointer-som.example.com
```

`stop standalone-server` stops both the dev and release copies of that mode. To stop only the release copy:

```bash
npm run quick-deploy -- stop standalone-server --runtime bin
```

## Print the plan without starting

```bash
npm run quick-deploy -- plan standalone-server
npm run quick-deploy -- plan standalone-server --runtime bin
npm run quick-deploy -- plan managed-client --api-base https://pointer-api.example.com --web-base https://pointer.example.com --annotate-base https://pointer-som.example.com
```

`stop` with no mode stops every mode and both runtimes recorded under `.quick-deploy/`. It signals the pids in the pid file and does not kill processes by name.
