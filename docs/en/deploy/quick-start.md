# Start locally

[简体中文](../../zh-CN/deploy/quick-start.md) | English

Developers and users do not start Pointer the same way.

## Start with the script

Users start the desktop app or the Web Server with the script. The script does not call npm. Omitting `--runtime` means `--runtime bin`.

```bash
./scripts/quick-deploy.sh start standalone-client --runtime bin
```

```bat
scripts\quick-deploy.bat start standalone-client --runtime bin
```

An existing `target/release/pointer-app` in the repo opens directly. Without that file, the installed `pointer-app` is used. When neither exists, the script exits. A local recompile happens only when you pass `--build`:

```bash
./scripts/quick-deploy.sh start standalone-client --runtime bin --build
```

After `--build`, the process that starts is the new `target/release/pointer-app`, not the installed app. Data lives in `.quick-deploy/bin/standalone-client/data`, separate from an installed Pointer.

To open an already built server in the browser:

```bash
./scripts/quick-deploy.sh start standalone-server --runtime bin
```

The page is at `http://127.0.0.1:8787/`. The first start writes the local account to `.quick-deploy/bin/standalone-server/standalone-admin.env` and prints the password once. Later starts reuse that file and do not print the password. A self-hosted server needs an existing `pointer-server` and `dist/`. If either is missing, the script exits. `--build` is what compiles locally (`node scripts/build-server.mjs`) and then starts that binary.

## Changing the code

Developers start the dev app with npm. That compiles as the code changes. Run the commands from the repository root. With no `pointer.local.env`, no control plane is bound. Requirements, debugging, and the libraries to install when this fails are in [DEVELOPMENT.md](../DEVELOPMENT.md).

### Client

```bash
npm install
npm run tauri:dev
```

That opens the desktop window. The first compile can take a few minutes.

### Web Server

Use two terminals:

```bash
# terminal 1
npm install
npm run server:dev

# terminal 2
npm run web:dev
```

Open `http://127.0.0.1:1420/`.

## Other cells

Packaging and rollout stay in [README.md](README.md). The rest of this page is the other cells of the same launcher. It does not read `pointer.local.env`. Each mode sets its own environment, so a managed file on the machine cannot leak into a standalone run. State, logs, and the generated local password live in `.quick-deploy/` (gitignored). Use `./scripts/quick-deploy.sh` on Linux and macOS, and `scripts\quick-deploy.bat` on Windows. Binary state is `.quick-deploy/bin/<mode>/`. Dev state is `.quick-deploy/<mode>/`. The two runtimes do not share a password or a pid file.

| `--runtime` | Default | What it launches | Rebuilds? |
| --- | --- | --- | --- |
| `bin` | yes | Desktop client: an existing `target/release/pointer-app`, otherwise the installed `pointer-app`. Server: an existing `pointer-server` | no. If the client is missing in both places, or the server is missing its binary or `dist/`, it exits. Only `--build` compiles locally |
| `dev` | no | `cargo run`, the local Vite CLI, or the local Tauri CLI | only when sources, `POINTER_EDITION`, or the three domains changed. Otherwise Cargo reuses the existing debug artifacts |

Day-to-day code changes use the npm commands above, not `--runtime dev`. `--runtime dev` is only for launching one of the four cells as a dev process through this script.

| Mode | After `--runtime bin` | After `--runtime dev` |
| --- | --- | --- |
| `standalone-server` | Page at `http://127.0.0.1:8787/`, `GET /api/auth/mode` is `standalone` | Vite page at `http://127.0.0.1:1420/`, API still on `8787` |
| `standalone-client` | Desktop window is `pointer-app` and does not use `1420` | Vite listens on `127.0.0.1:1420` and the desktop window opens |
| `managed-server` | Same as the standalone server, and `GET /api/auth/mode` is `platform` | Same, with `POINTER_EDITION=managed` |
| `managed-client` | Desktop process receives the three control-plane domains and does not use `1420` | `tauri dev` with the three domains present at compile and run time |

The dev server web UI and the dev desktop client both use Vite port `1420`, so only one of them can run. The server API stays on `127.0.0.1:8787`. A release server serves `dist/` itself at `http://127.0.0.1:8787/`. Dev and release servers cannot listen on `8787` at the same time.

## Standalone server

```bash
./scripts/quick-deploy.sh start standalone-server --runtime bin
```

```bat
scripts\quick-deploy.bat start standalone-server --runtime bin
```

Sign in with the account, the password, and the captcha. The model key is still filled in from settings.

```bash
./scripts/quick-deploy.sh status
./scripts/quick-deploy.sh stop standalone-server --runtime bin
```

## Standalone client

```bash
./scripts/quick-deploy.sh start standalone-client --runtime bin
```

A local standalone build leaves `POINTER_EDITION` unset. A standalone `--build` locks the edition and the domains, so `pointer.local.env` cannot fill them. An installed official package still receives its model from the platform after sign-in.

## Managed server

The three control-plane domains are required. Without a reachable control plane the process still starts in platform mode, but browser sign-in waits until the control plane is reachable.

```bash
./scripts/quick-deploy.sh start managed-server --runtime bin \
  --api-base https://pointer-api.example.com \
  --web-base https://pointer.example.com \
  --annotate-base https://pointer-som.example.com
```

```bat
scripts\quick-deploy.bat start managed-server --runtime bin --api-base https://pointer-api.example.com --web-base https://pointer.example.com --annotate-base https://pointer-som.example.com
```

You can export `POINTER_API_BASE`, `POINTER_WEB_BASE`, and `COMPUTER_ANNOTATE_API_BASE` instead of passing the three flags. Without `--build`, the script puts the domains into the process environment and starts the existing binary. `--build` recompiles locally with the domains from this command. A build that is missing one domain fails. Developers can also produce this package with `npm run server:build`.

`--runtime dev` is what runs `cargo run -p pointer-server` and the local Vite CLI. Cargo rebuilds only when the sources, `POINTER_EDITION`, or the three domains change.

## Managed client

```bash
./scripts/quick-deploy.sh start managed-client --runtime bin \
  --api-base https://pointer-api.example.com \
  --web-base https://pointer.example.com \
  --annotate-base https://pointer-som.example.com
```

Without `--build`, an existing `target/release/pointer-app` or the installed app is used. `--build` recompiles locally with the three domains from this command. An enterprise client also needs an updater private key, defaulting to `~/.tauri/pointer-updater.key` (Windows: `%USERPROFILE%\.tauri\pointer-updater.key`). When the file is missing, a terminal asks once and generates it only after `y`. `--generate-updater-key` generates it without asking. An existing file is kept. Developers can also produce this client with `npm run tauri:build`.

## Plan without starting

```bash
./scripts/quick-deploy.sh plan standalone-server
./scripts/quick-deploy.sh plan standalone-client --runtime dev
./scripts/quick-deploy.sh plan managed-client --api-base https://pointer-api.example.com --web-base https://pointer.example.com --annotate-base https://pointer-som.example.com
```

`plan` without `--runtime` prints the binary plan. `stop` without a mode stops every recorded mode and both runtimes under `.quick-deploy/`. `stop <mode>` without `--runtime` stops that mode's binary and its dev runtime. Stop signals the pid file. It does not kill by process name.
