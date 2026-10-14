# Local quick start for the four deployment cells

[简体中文](../../zh-CN/deploy/quick-start.md) | English

Packaging and rollout stay in [README.md](README.md). This page starts the same kind of local smoke processes used to check that one deployment cell actually comes up.

The entry point is `scripts/quick-deploy.mjs` at the repo root (`npm run quick-deploy -- …`). It does not read `pointer.local.env`. Each mode sets its own environment, so a managed file on the machine cannot leak into a standalone run. State, logs, and the generated local password live in `.quick-deploy/` (gitignored).

| Mode | What it starts | How to tell it worked |
| --- | --- | --- |
| `standalone-server` | `cargo run -p pointer-server` plus `npm run web:dev`, with a generated local account | `GET /api/auth/mode` is `standalone`; open `http://127.0.0.1:1420/` |
| `standalone-client` | `npx tauri dev`, data under `.quick-deploy/standalone-client/data` | Vite listens on `127.0.0.1:1420` and the desktop window opens |
| `managed-server` | The same server and web processes, with `POINTER_EDITION=managed` and `POINTER_DEPLOYMENT_MODE=platform` | `GET /api/auth/mode` is `platform` |
| `managed-client` | `tauri dev` with the three control-plane domains present at compile and run time | The desktop process starts as the managed edition |

The server web UI and the desktop client both use Vite port `1420`, so only one of them can run. The server API stays on `127.0.0.1:8787`, which is what the Vite `/api` proxy expects.

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

## Print the plan without starting

```bash
npm run quick-deploy -- plan standalone-server
npm run quick-deploy -- plan managed-client --api-base https://pointer-api.example.com --web-base https://pointer.example.com --annotate-base https://pointer-som.example.com
```

`stop` with no mode stops every mode recorded under `.quick-deploy/`. It signals the pids in the pid file and does not kill processes by name.
