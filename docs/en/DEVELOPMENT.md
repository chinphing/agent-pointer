# Development and debugging notes

English | [简体中文](../../DEVELOPMENT.md)

This document describes the local development, debugging, checking and troubleshooting workflow for `Pointer`.

## Requirements

### Common dependencies

- Node.js 20+
- npm
- Rust stable
- Tauri 2 CLI, provided by the project dependencies

Check commands:

```bash
node -v
npm -v
rustc --version
cargo --version
```

### Extra requirements on Windows

- Microsoft C++ Build Tools
- WebView2 Runtime

### Extra requirements on macOS

```bash
xcode-select --install
```

### Extra requirements on Linux

Ubuntu/Debian:

```bash
sudo apt-get update
sudo apt-get install -y \
  libwebkit2gtk-4.1-dev \
  build-essential \
  pkg-config \
  curl \
  wget \
  file \
  libxdo-dev \
  libssl-dev \
  libayatana-appindicator3-dev \
  librsvg2-dev \
  libpipewire-0.3-dev \
  libspa-0.2-dev \
  libclang-dev \
  libgbm-dev \
  libegl1-mesa-dev \
  libdrm-dev \
  libwayland-dev
```

## Installing dependencies

Run this in the project root:

```bash
npm install
```

When `POINTER_EDITION` is not set, no control plane is bound (standalone, purely local). To integrate with a control plane, set `POINTER_EDITION=managed` and the three domains in `pointer.local.env` (see [deploy/editions.md](deploy/editions.md)); writing any other value (including the old `official`) makes the build fail outright.

## Starting development mode

### Desktop

Full desktop app debugging:

```bash
npm run tauri:dev
```

or:

```bash
npm run tauri dev
```

This starts, at the same time:

- the Vite frontend dev server
- the Tauri desktop window
- the Rust backend service

**Running alongside the release version:** Debug builds (`tauri dev` / `cargo run`) use a separate data directory **`PointerAppDev`** by default, fully isolated from the installed **`PointerApp`** (conversations, settings, sign-in state, task board and so on do not affect each other). Path examples for the three platforms:

| Platform | Release | Development |
|------|--------|--------|
| **macOS** | `~/Library/Application Support/PointerApp/` | `…/PointerAppDev/` |
| **Linux** | `$XDG_DATA_HOME/PointerApp/` (default `~/.local/share/PointerApp/`) | `…/PointerAppDev/` |
| **Windows** | `%APPDATA%\PointerApp\` | `%APPDATA%\PointerAppDev\` |

The first `tauri dev` creates an empty directory; you need to configure the model and sign in again in the development instance.

Optional environment variables (highest priority first):

| Variable | Effect |
|------|------|
| `POINTER_APP_DATA_DIR` | Absolute path, fully specifies the data directory |
| `POINTER_APP_DATA_SUBDIR` | Overrides the subdirectory name under `data_dir()` (e.g. `PointerAppDev`) |

Release builds (`tauri:build` output) always use **`PointerApp`** unless the environment variables above are set.

**Attachments and local media paths:** resolved uniformly through `pointer_core::media::resolve_local_media_path`. A persisted attachment's `storageRelPath` (of the form `{conv}/{id}_{fileName}`) is looked up only under `{app_data}/conversation-media/` and is **never** resolved relative to the process `cwd` (e.g. `src-tauri/`), avoiding a mismatch between dev and the data directory.

### Web

The web client reuses the same Vue UI and the backend reuses `crates/pointer-core`, with the `server` crate providing the HTTP/SSE API.

Start the Rust web backend:

```bash
npm run server:dev
```

In another terminal, start the web frontend:

```bash
npm run web:dev
```

Same origin by default: `WEB_API_BASE` is empty and Vite proxies `/api` to `127.0.0.1:8787` (`server:dev` must be running first). pointer-server CORS is off by default; `tauri:dev` does not start that proxy (the desktop goes through IPC).

If the frontend talks to the API directly (cross-origin), CORS must be enabled as well:

```bash
POINTER_SERVER_CORS_ORIGINS=* npm run server:dev
VITE_WEB_API_BASE=http://127.0.0.1:8787 npm run web:dev
```

## Debugging the frontend alone

```bash
npm run dev
```

Then open the local address printed by the terminal.

Note: when running in a browser alone, Tauri desktop capabilities such as `invoke`, event listeners and file storage are unavailable; for the full feature set use `npm run tauri:dev`.

## Opening developer tools

In the Tauri window you can open the frontend DevTools with:

- Windows/Linux: `Ctrl + Shift + I`
- macOS: `Cmd + Option + I`

Useful for inspecting:

- Console logs
- Network requests
- Vue runtime errors
- Tauri `invoke` call errors
- Streaming event handling state

## Model configuration debugging

After the first launch, fill in the DashScope API Key in the app's "Settings".

Default configuration:

```text
Provider: qwen
Base URL: https://dashscope.aliyuncs.com/compatible-mode/v1
Model: qwen-plus
```

Suggested debugging flow:

1. Open Settings
2. Fill in the DashScope API Key
3. Click Test connection
4. Send a simple message, e.g. `Hello`
5. Test a tool call, e.g. `calculate 123 * 456 for me`

## Debugging external Skills

External Skills strictly follow the official Claude Skills directory convention. Load sources by priority, highest first:

1. `~/.pointer/skills/` (user library; install with **`skill_import`**, edit by delegating to a **coder** sub-agent)
2. `~/.agents/skills/` (Codex / Agent compatible, read-only)
3. `{data_dir}/PointerApp/skills/` (bundled system library)

The workspace `skills/` is not part of runtime loading. See `docs/user/skills.md` (users) and `docs/developer/skills-compatibility.md` (format specification).

The Windows app data directory is usually:

```text
C:\Users\<username>\AppData\Roaming\PointerApp\skills
```

How to debug:

1. Prepare a zip containing a kebab-case Skill directory and an exactly named `SKILL.md`.
2. Start the desktop or web client.
3. Open the "Skills library".
4. Click "Import zip".
5. After import the list shows an "External" marker.

`SKILL.md` example:

```markdown
---
name: translator
description: Translates and standardizes terminology between Chinese and English. Use when users ask for translation, polishing, or terminology consistency.
metadata:
  tags:
    - translation
---

# Translator Skill

Use this when the user asks for translation or polishing. Keep the original meaning and produce natural, accurate wording in the target language.
```

Notes: `name` and `description` are the first-layer frontmatter index, and `name` must match the Skill directory name; `skill.md`, `skill.json`, `manifest.json` and the old fields `id`, `systemPrompt`, `toolNames` are not supported. After a Skill is enabled the full body is not injected immediately; when needed the model reads the second-layer `SKILL.md` body through **`skill_read`** (`skill_id` + required `path`, `SKILL.md` when reading the instructions); files under `references/`, `assets/`, `scripts/` are third-layer resources read on demand through **`skill_read`** (same `skill_id` + resource-relative `path`). No arbitrary code from the zip is executed.




## Rust backend debugging


Enter the Tauri backend directory:

```bash
cd src-tauri
```

Check Rust compilation:

```bash
cargo check
```

For unit-level compile checks it is best to first go back to the project root and run the full development command:

```bash
npm run tauri:dev
```

Enable verbose logging:

### PowerShell

```powershell
$env:RUST_LOG="debug"; npm run tauri:dev
```

### Bash/zsh

```bash
RUST_LOG=debug npm run tauri:dev
```

## Frontend checks

Type checking and frontend build:

```bash
npm run build
```

If you only need to start Vite:

```bash
npm run dev
```

## Icons and packaging

Cross-platform environment, development, packaging and CI releases are in **[`contributing/cross-platform-build.md`](contributing/cross-platform-build.md)**.

Quick check before packaging:

```bash
npm run icons
npm run tauri:build          # current system
# or: build:windows / build:macos / build:linux (must run on the matching OS)
```

Artifacts: `src-tauri/target/release/bundle/` (Windows `msi/`, macOS `dmg/`+`macos/`, Linux `deb/`+`appimage/`).

## FAQ

### 1. The `tauri` command is not found

Install the dependencies first:

```bash
npm install
```

Then use the project script:

```bash
npm run tauri:dev
```

Do not rely on a global `tauri` command.

### 2. `npx` or `npm` is not found

This means Node.js is not installed or not on PATH. Install Node.js 20+ and reopen the terminal.

### 3. Windows build fails, reporting a missing C++ toolchain

Install Microsoft C++ Build Tools and tick the C++ desktop development components.

### 4. Linux build fails, reporting a missing WebKitGTK

Install the Linux system dependencies; see the Linux section of [`contributing/cross-platform-build.md`](contributing/cross-platform-build.md).

### 5. API requests fail

Check:

- Whether the API Key is filled in
- Whether the Base URL is `https://dashscope.aliyuncs.com/compatible-mode/v1`
- Whether the model name is available, e.g. `qwen-plus`
- Whether the network can reach the DashScope service

### 6. Tool calls do not respond

Check the frontend DevTools Console and the terminal logs, focusing on:

- `chat://stream` events
- `tool_call_start`
- `tool_call_result`
- `approve_tool_call`

## Recommended development workflow

```bash
npm install
npm run tauri:dev
```

After changing code:

```bash
npm run build
cd src-tauri
cargo check
```

Before packaging, see [`contributing/cross-platform-build.md`](contributing/cross-platform-build.md):

```bash
npm run icons
```

### 7. `vue-tsc`: `number` is not assignable to `Timeout`

When a project pulls in both the DOM and `@types/node`, `ReturnType<typeof setTimeout>` becomes Node's `Timeout`, whereas `window.setTimeout` / `window.setInterval` return `number`.

Store frontend browser timer handles as `number` (or `ReturnType<typeof window.setTimeout>`), consistent with the existing `saveTimer` / `composerDraftTimer` style.

## Documentation directory conventions

| Path | Reader | Content |
|------|------|------|
| [`user/`](user/README.md) | Pointer end users | Installation, Skills, IM, cloud host, sub-agent settings |
| [`developer/`](developer/README.md) | External developers, Skill authors | Channel deployment, Skill format, extension hooks, protocols |
| [`contributing/`](contributing/README.md) | Repository contributors | Cross-platform builds, platform UI, offline evaluation |
| [`internals/`](internals/README.md) | Core maintainers | Runtime internals, task board, etc. |
| [`design/`](design/README.md) | Review / planning | Design proposals, roadmaps, implementation plans |

When adding a document, decide the reader first: user tutorials → `user/`; third-party integration → `developer/`; development of this repository → `contributing/` or `internals/` / `design/`.

```bash
npm run tauri:build
```
