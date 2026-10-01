# Cross-platform development and packaging

English | [简体中文](../../contributing/cross-platform-build.md)

This document collects environment preparation, local development, compilation/packaging and the CI release flow on **Windows / macOS / Linux**.  
The working directory is always **`agent-pointer/`** inside the repository (the Tauri + Vue project root).

> For the platform packaging quick reference (environment prerequisites / build commands / artifact locations / CI policy) see [`../deploy/platforms.md`](../deploy/platforms.md); for the packaging and deployment entry point see [`../deploy/README.md`](../deploy/README.md).

---

## Architecture overview

```text
Vue unified UI
├─ Desktop: Tauri Adapter → src-tauri → crates/pointer-core
└─ Web: Web Adapter → server (axum HTTP/SSE) → crates/pointer-core
```

| Capability | Desktop | Web | License issuance |
|------|--------|--------|--------------|
| Development | `npm run tauri:dev` | `npm run server:dev` | `npm run license-gen:dev -- …` |
| Packaging | `npm run tauri:build` | `npm run server:build` | `npm run license-gen:build` |
| Artifacts | Installer for the current platform | zip (all platforms) + deb (automatic on Linux) | zip (all platforms) |

Every module follows the same **`{module}:dev` / `{module}:build`** naming; the build script picks the artifact format from the current OS (the same idea as `tauri:build`).

---

## Common dependencies

Required on every platform:

| Tool | Suggested version | Check command |
|------|----------|----------|
| Node.js | 20+ | `node -v` |
| npm | Ships with Node | `npm -v` |
| Rust | **stable ≥ 1.94** (the lower bound of Cursor's bundled rust-analyzer) | `rustc --version` / `cargo --version` |

This repository's `rust-toolchain.toml` tracks rustup `stable` and installs `rustfmt` / `clippy` / `rust-analyzer`. If the IDE reports the toolchain as too old, run this at the project root:

```bash
rustup update stable
rustup component add rust-analyzer rustfmt clippy
```

Then reload rust-analyzer (Command Palette → **Rust Analyzer: Restart server**). Do not analyse Cursor with a rustc older than 1.94.

The Tauri CLI comes from the project devDependency — **do not rely on a global `tauri` command**; always use `npm run tauri:*`.

When compiling, `pointer-core` builds the **sqlite-cjk-fts** native extension (the `cjk_bigram` FTS5 tokenizer) used by `session_search` for Chinese search. A local C compiler is required:

| Platform | Requirement |
|------|------|
| macOS / Linux | `cc` or `gcc` (Xcode CLT / build-essential) |
| Windows | MSVC `cl` or MinGW `gcc` |

The extension is **statically linked** into `pointer-core` at build time and registered through `sqlite3_auto_extension`; no DLL has to be released at runtime.

**Conversation history** lives in `{data_dir}/PointerApp/conversations.db` (SQLite WAL, Hermes-style write retry + incremental upsert). The first upgrade imports it automatically from `conversations.json`. The old `sessions.db` index database is deprecated.

First time in the project:

```bash
cd agent-pointer
npm install
```

---

## Windows

### Requirements

- **Microsoft C++ Build Tools** (tick "Desktop development with C++")
- **LLVM (libclang)** — needed **only when building from source**; the `silk-v3-sys` build script generates C bindings through bindgen. End users installing the MSI **do not** need LLVM. See "Installing LLVM / libclang" below.
- **WebView2 Runtime** (usually preinstalled on Win10/11; install the Evergreen Bootstrapper from Microsoft's site when a dev machine lacks it)

### Installing LLVM / libclang (developers)

On Windows, bindgen needs `libclang.dll` (or `clang.dll`). Pick either installation method and **open a new terminal afterwards**.

**Option 1: winget (recommended)**

```powershell
winget install LLVM.LLVM
```

Default path: `C:\Program Files\LLVM\bin\libclang.dll`

**Option 2: official installer**

Download the **Windows 64-bit** installer from [LLVM Releases](https://github.com/llvm/llvm-project/releases) (for example `LLVM-19.x.x-win64.exe`) and tick **Add LLVM to the system PATH** while installing.

**Configure the environment variable**

If you still get `Unable to find libclang` after installing, set it in PowerShell (adjust the path to your actual installation directory):

```powershell
# current terminal session
$env:LIBCLANG_PATH = "C:\Program Files\LLVM\bin"

# persist to the user environment variables (open a new terminal afterwards)
[Environment]::SetEnvironmentVariable("LIBCLANG_PATH", "C:\Program Files\LLVM\bin", "User")
```

**Verify**

```powershell
Test-Path "C:\Program Files\LLVM\bin\libclang.dll"   # should print True
$env:LIBCLANG_PATH = "C:\Program Files\LLVM\bin"
npm run tauri:build
```

If LLVM is installed under Visual Studio's Clang component, the usual path is:

`C:\Program Files\Microsoft Visual Studio\2022\BuildTools\VC\Tools\Llvm\x64\bin`

### Development

```powershell
cd agent-pointer
npm install
npm run tauri:dev
```

Open DevTools: `Ctrl + Shift + I`

The web frontend is same-origin by default (Vite proxies `/api` to 8787). For cross-origin, set the API address and enable CORS (PowerShell):

```powershell
$env:POINTER_SERVER_CORS_ORIGINS="*"; npm run server:dev
# in another terminal:
$env:VITE_WEB_API_BASE="http://127.0.0.1:8787"; npm run web:dev
```

### Packaging

```powershell
npm run icons          # first time, or after replacing icon.png
npm run build:windows  # same as npm run tauri:build (Windows does not set NO_STRIP)
```

**Artifact directory:**

```text
src-tauri/target/release/bundle/
└── msi/               # *.msi installer
```

**Release binary:**

```text
src-tauri/target/release/pointer-app.exe
```

### Troubleshooting

| Symptom | Fix |
|------|------|
| Missing C++ toolchain | Install [Microsoft C++ Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/) and tick **"Desktop development with C++"**; open a **new** terminal before compiling again |
| `failed to run C compiler cl` / `cjk fts` compilation failure | Same as above; or run `npm run tauri:build` from an **x64 Native Tools Command Prompt** that already has MSVC configured. With a MinGW toolchain you can set `CC=gcc` |
| `Unable to find libclang` (`silk-v3-sys` / bindgen) | Install LLVM as described in "Installing LLVM / libclang", set `LIBCLANG_PATH` to the `bin` directory containing `libclang.dll`, and rebuild after **opening a new terminal** |
| `tauri` not found | Use `npm run tauri:dev`, and run `npm install` first |
| Computer control does not respond | Check screen-recording / accessibility permissions; test with a release package |

### Official release (optional)

- Configure **Authenticode code signing** to avoid SmartScreen blocking
- Application data directory: `%APPDATA%\PointerApp\`

---

## macOS

### Requirements

```bash
xcode-select --install
```

**Node.js 20+** and **Rust stable** are required. Packaging `.app` / `.dmg` **must be built on macOS** (a distributable macOS package cannot be cross-compiled).

**Minimum system version:** macOS **10.15 (Catalina)** (see `src-tauri/tauri.conf.json` → `bundle.macOS.minimumSystemVersion`).

### Development

```bash
cd agent-pointer
npm install
npm run tauri:dev
```

Open DevTools: `Cmd + Option + I`

### Packaging

```bash
npm run icons
npm run build:macos              # unsigned (native CPU architecture, as before)
npm run build:macos:universal    # unsigned, Universal Binary (Intel + Apple Silicon)
npm run build:macos:sign-only    # Universal + sign only, no notarisation (the default for signed packaging)
npm run build:macos:signed       # Universal + Developer ID signing + notarisation
```

**Signing + notarisation (official release):**

```bash
# interactively generate signing/macos/signing.env (p12 path, password, Team ID, Apple ID, …)
npm run signing:macos:setup

# signed packaging defaults to a Universal Binary (Intel + Apple Silicon)
npm run build:macos:signed
```

See [`signing/macos/README.md`](../../../signing/macos/README.md).  
`signing.env` and `*.p12` are gitignored — never commit them to the repository.

A signed Universal build needs both Rust architecture targets (`build:macos:sign-only` / `build:macos:signed` run `rustup target add` automatically). For an unsigned Universal build, do it manually:

```bash
rustup target add aarch64-apple-darwin x86_64-apple-darwin
npm run build:macos:universal
```

**Artifact directory:**

```text
target/release/bundle/                    # build:macos (native architecture)
target/universal-apple-darwin/release/bundle/   # signed packaging / build:macos:universal
├── macos/             # *.app
└── dmg/               # Pointer_*_<arch>.dmg or Pointer_*_universal.dmg
```

### Computer control permissions

Computer control on macOS needs "Screen Recording + Accessibility".  
Under `tauri dev` the executable may not live inside a `.app`, so the permission wizard can be incomplete — **test with a packaged `.app`**.  
See [`docs/internals/macos-computer-permissions.md`](../../internals/macos-computer-permissions.md).

### "Damaged and can't be opened" prompt

For a `.dmg` / `.app` downloaded from a browser / cloud drive / chat tool that was **not signed with a Developer ID and notarised**, macOS Gatekeeper often reports "damaged" or "cannot verify the developer" — **the package is not actually broken**; the system is refusing to run an app from an untrusted source.

**User side (internal testing / self-built packages):**

1. **Recommended**: do not double-click the app straight from the DMG; drag `Pointer.app` into "Applications" and run this in a terminal (adjust the path to your actual one):
   ```bash
   xattr -cr /Applications/Pointer.app
   ```
   This removes the `com.apple.quarantine` download attribute; then open it from Launchpad or the Applications folder.

2. **Or**: in Finder, **right-click** `Pointer.app` → **Open** → click **Open** again in the dialog (needed only the first time).

3. If it still fails, check "System Settings → Privacy & Security" for an "Open Anyway" button.

**Distribution side (official release):** see "Signing and distribution" below — signing with an Apple Developer certificate plus completing Notarization is what removes those steps for users.

### Official release (optional)

- Apple Developer certificate signing
- **Notarization** before distributing
- Application data: `~/Library/Application Support/PointerApp/`

---

<a id="linux"></a>
## Linux (Ubuntu / Debian)

CI uses **ubuntu-24.04**. Note: the screen-recording dependency (xcap → pipewire/libspa 0.9) needs PipeWire ≥ 1.0 headers, and the 0.3.48 shipped with Ubuntu 22.04 cannot compile it; the Linux build baseline is therefore 24.04, and artifacts require glibc ≥ 2.39.

### System dependencies

```bash
sudo apt-get update
sudo apt-get install -y build-essential pkg-config
# or install the whole Tauri Linux packaging dependency set in one go:
# bash scripts/install-linux-build-deps.sh
```

A complete Tauri package additionally needs WebKitGTK, FUSE, GStreamer and more — see `scripts/install-linux-build-deps.sh` or the "Packaging" section below.

When you only compile Rust native dependencies (such as `xcap`), you can install these in addition:

```bash
sudo apt-get install -y \
  libwebkit2gtk-4.1-dev \
  libpipewire-0.3-dev \
  libspa-0.2-dev \
  libclang-dev \
  libgbm-dev \
  libegl1-mesa-dev \
  libdrm-dev \
  libwayland-dev \
  build-essential \
  ca-certificates \
  pkg-config \
  libxdo-dev \
  librsvg2-dev \
  libgdk-pixbuf-2.0-dev \
  libgtk-3-dev \
  libgtk-3-bin \
  patchelf
```

Notes:

- `build-essential`: provides the `gcc`/`cc` linker; without it you get **`linker cc not found`**
- `libwebkit2gtk-4.1-dev`, `libglib2.0-dev`: the Tauri desktop shell depends on GTK/WebKit; without them you get **`glib-sys` / `Package 'glib-2.0' not found`**
- `libpipewire-0.3-dev`, `libspa-0.2-dev`: needed to compile the Linux screenshot library `xcap` (without them you get `libspa-sys` / `libpipewire-0.3` not found)
- `libclang-dev`: `libspa-sys` needs `libclang.so` when generating C bindings through bindgen (without it you get `Unable to find libclang`)
- `libxdo-dev`: the computer-control agent (`enigo`) links against `libxdo`; without it you get **`unable to find library -lxdo`**
- Under Wayland, screenshot/input capabilities vary by compositor; for complex cases verify in an X11 session

**Windows Computer screenshots**: they go through xcap WGC (D3D11) by default. Under RDP, in a VM, or when GPU memory is low (`E_OUTOFMEMORY` / `0x8007000E`) WGC may panic; `pointer-core` catches that and automatically falls back to GDI BitBlt (`vision/windows_gdi.rs`). If both fail, close GPU-heavy programs or retry on a local desktop (not a remote session).

Distributions such as **Fedora / RHEL** need to install the equivalently named WebKitGTK 4.1, ayatana-appindicator, librsvg development packages themselves; this document uses Ubuntu as the reference.

### Installing Node.js and Rust (if the system lacks them)

```bash
# Node.js 20
curl -fsSL https://deb.nodesource.com/setup_20.x | sudo -E bash -
sudo apt-get install -y nodejs

# Rust
export RUSTUP_DIST_SERVER=https://mirrors.tuna.tsinghua.edu.cn/rustup
export RUSTUP_UPDATE_ROOT=https://mirrors.tuna.tsinghua.edu.cn/rustup/rustup
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source "$HOME/.cargo/env"
```

### Development

```bash
cd agent-pointer
npm install
npm run tauri:dev
```

Open DevTools: `Ctrl + Shift + I`

### Packaging

```bash
npm run icons
npm run build:linux    # equivalent to npm run tauri:build
```

**Artifact directory:**

```text
src-tauri/target/release/bundle/
├── deb/               # *.deb
└── appimage/          # *.AppImage
```

**Building one format only:**

```bash
npm run tauri:build -- --bundles deb
npm run tauri:build -- --bundles appimage   # on Linux `scripts/tauri-build.mjs` sets NO_STRIP=true automatically
# equivalent shortcut script:
npm run build:linux:appimage
```

**Installing the deb:**

```bash
sudo dpkg -i src-tauri/target/release/bundle/deb/*.deb
sudo apt-get install -f
```

**Running the AppImage:**

```bash
chmod +x src-tauri/target/release/bundle/appimage/*.AppImage
./src-tauri/target/release/bundle/appimage/*.AppImage
```

**Release binary:**

```text
src-tauri/target/release/pointer-app
```

### Troubleshooting

| Symptom | Fix |
|------|------|
| `failed to run linuxdeploy` (AppImage stage) | ① **when the deb already succeeded** you can use `bundle/deb/*.deb` for now; ② run `bash scripts/install-linux-build-deps.sh` (it includes `libfuse2`, `squashfs-tools`, `patchelf`, `file`); ③ you **must** use `npm run tauri:build` (it sets `NO_STRIP=true` + `APPIMAGE_EXTRACT_AND_RUN=1` automatically), not `cargo tauri build`; ④ for details: `npm run tauri:build -- --bundles appimage --verbose` (commonly `Strip call failed` / `.relr.dyn` → confirm `NO_STRIP=true`) |
| `no 'libdir' variable for 'librsvg-2.0'` / gtk plugin exit 1 | Install `librsvg2-dev libgdk-pixbuf-2.0-dev libgtk-3-dev libgtk-3-bin`; verify that `pkg-config --variable=libdir librsvg-2.0` prints output |
| WebKitGTK not found | Confirm that `libwebkit2gtk-4.1-dev` is installed |
| `libspa-sys` / `libpipewire-0.3` not found | Install `libpipewire-0.3-dev` and `libspa-0.2-dev`, then re-run `npm run tauri:build` |
| `Unable to find libclang` (bindgen) | Install `libclang-dev` (or `clang`); if needed `export LIBCLANG_PATH=/usr/lib/llvm-*/lib` |
| `unable to find library -lgbm` (linking) | Install `libgbm-dev libegl1-mesa-dev libdrm-dev libwayland-dev` |
| `unable to find library -lxdo` (linking) | Install `libxdo-dev` (already included in `scripts/install-linux-build-deps.sh`) |
| Computer control misbehaves | Wayland limitation; try X11; confirm `libxdo-dev` is installed |
| First compile is extremely slow | Normal — a full Rust release compile takes about 10–30 minutes |

---

## Web development and deployment

The web side reuses the same Vue UI; `pointer-core` is served over HTTP/SSE by the `server` crate.

### Local development

Terminal 1 — backend:

```bash
cd agent-pointer
npm run server:dev
# default http://127.0.0.1:8787
```

Terminal 2 — frontend:

```bash
npm run web:dev
# default http://0.0.0.0:1420
```

`web:dev` is same-origin by default (Vite `/api` → `127.0.0.1:8787`); pointer-server CORS is off by default. Optional:

```bash
POINTER_SERVER_ADDR=0.0.0.0:8787 npm run server:dev
# enable this only for cross-origin:
# POINTER_SERVER_CORS_ORIGINS=* npm run server:dev
# VITE_WEB_API_BASE=http://127.0.0.1:8787 npm run web:dev
```

### Production build (illustrative)

**Split frontend/backend** (API and static pages deployed separately):

```bash
npm run build                              # Vue → dist/
cargo build -p pointer-server --release    # target/release/pointer-server
# static pages are hosted by Nginx etc.; set VITE_WEB_API_BASE=https://api.example.com when building
# and configure cors_origins in pointer-server (CORS is off by default)
```

**All-in-one frontend/backend** (recommended: a single process serves both the API and the Web UI):

```bash
npm run server:dev      # development
npm run server:build    # packaging (automatic: zip on all platforms; additionally .deb on Linux)
npm run server:start    # run in the foreground (development / debugging)
npm run server:daemon   # background daemon (production; syncs scripts into target/release)
npm run server:stop     # stop the daemon
npm run server:restart  # restart the daemon
npm run server:status   # show running state
# or directly ./target/release/pointer-server
```

`server:build` reads `pointer.local.env` at the repository root (pre-existing OS / CI variables win) and hands the same set of variables to both the Vue build and `cargo`; before packaging it verifies that the two halves agree on the flavour — under the managed flavour, a binary missing control-plane domains or web assets missing the web base fails the build outright, and no half-bound package is produced. For flavours and variables see [editions.md](../deploy/editions.md).

Standalone web frontend development (pointing at a remote or local server API):

```bash
npm run web:dev
```

After unpacking a deployment package you can also use the scripts in the same directory as the binary (no Node.js required):

```bash
./start.sh      # Linux / macOS
./stop.sh
./restart.sh
./status.sh
```

Windows (PowerShell):

```powershell
.\start.ps1
.\stop.ps1
.\restart.ps1
.\status.ps1
```

`server:build` produces a deployment package when it finishes:

```text
target/release/pointer-server-bundle/pointer-server-{platform}-{arch}.zip
└── pointer-server/
    ├── pointer-server[.exe]
    ├── dist/
    ├── skills/              # bundled default skills (synced to the data directory on startup)
    ├── pointer-server.toml.example
    ├── start.sh / stop.sh / restart.sh / status.sh
    └── start.ps1 / stop.ps1 / restart.ps1 / status.ps1
```

After unpacking, enter the `pointer-server/` directory, copy `pointer-server.toml.example` to `pointer-server.toml`, adjust it as needed, and start the binary.

Repackaging the zip only (without recompiling):

```bash
npm run server:package
```

**Automatic per-platform artifacts (one command):**

| Current platform | `npm run server:build` artifacts |
|----------|----------------------------|
| macOS | `pointer-server-macos-{arch}.zip` |
| Windows | `pointer-server-windows-{arch}.zip` |
| Linux | the zip above + `pointer-server_0.1.0_*.deb` (needs `dpkg-deb`) |

Repackaging the zip only (without recompiling):

```bash
npm run server:package
```

Rebuilding the deb only (Linux, needs an existing release binary):

```bash
npm run server:build:deb
```

Open `http://127.0.0.1:8787` in a browser (API and pages on the same port).

`dist/` lookup order: `POINTER_SERVER_STATIC_DIR` → `dist/` in the current directory → `dist/` next to the executable → `target/release/../../dist`.

**Configuration file (recommended for Windows / NSSM deployments)**: place `pointer-server.toml` or `pointer-server.env` next to `pointer-server.exe` and it is loaded automatically at startup; pre-existing OS environment variables win. See `server/pointer-server.toml.example`.

```toml
# pointer-server.toml
[server]
addr = "0.0.0.0:8787"
static_dir = "dist"          # relative paths are relative to the directory holding the config file
skills_dir = "skills"        # bundled Skills source; for deb use /usr/share/pointer-server/skills
# data directory and logs default to the same as the desktop client (PointerApp/logs/, PointerAppDev for debug); no configuration needed

[pointer]
api_base = "https://pointer-api.readflowai.com"
oauth_client_secret = "your-secret"
```

You can also use the dotenv-format `pointer-server.env`, or point at a path with `POINTER_SERVER_CONFIG=C:\pointer\pointer-server.toml`.

Environment-variable overrides are still supported (NSSM's `AppEnvironmentExtra` overrides same-named entries in the file):

```bash
POINTER_SERVER_ADDR=0.0.0.0:8787 npm run server:start
POINTER_SERVER_STATIC_DIR=/opt/pointer/dist npm run server:start
```

The web side does not offer full computer control; desktop capabilities (`invoke`, local storage, …) are available only inside Tauri.

---

## Common packaging flow

Whichever the platform, the recommended order is:

```bash
cd agent-pointer
npm install
npm run icons          # first time, or after replacing icon.png
npm run tauri:build      # or build:windows / build:macos / build:linux
```

The build automatically runs `beforeBuildCommand` (`npm run build`: Vue type check + Vite bundle into `dist/`).

**Unified artifact root:** (only the subdirectory for the platform appears on the **current build system**)

```text
src-tauri/target/release/bundle/
├── msi/          # Windows
├── macos/        # macOS (.app)
├── dmg/          # macOS
├── deb/          # Linux
└── appimage/     # Linux (AppImage)
```

| Platform | Recommended command |
|------|----------|
| Windows | `npm run build:windows` |
| macOS | `npm run build:macos` (native architecture; unsigned Universal: `build:macos:universal`) |
| Linux | `npm run build:linux` |

**Product name:** Pointer (`src-tauri/tauri.conf.json` → `productName`)

### Related configuration files

| File | Purpose |
|------|------|
| `src-tauri/tauri.conf.json` | Window, bundle targets, Linux deb/AppImage, Windows NSIS |
| `package.json` | `tauri:dev` / `tauri:build` / `build:*` / `icons` scripts |
| `scripts/tauri-build.mjs` | Cross-platform `tauri build`; reads `TAURI_SIGNING_PRIVATE_KEY_PATH`; the `managed` flavour bakes in control-plane domains, standalone automatically adds `--config src-tauri/tauri.personal.conf.json`; **Linux only** sets `NO_STRIP=true` automatically (AppImage) |
| `.github/workflows/release.yml` | Three-platform CI packaging (**standalone packages only**) |

### Icons

```bash
npm run icons
# or specify a source image: node scripts/generate-app-icons.mjs /path/to/logo.png
```

Generates rounded icons from the source PNG (with a 1254px source the corner radius is 250px, transparent corners) and produces the sizes for each platform:
- **Windows / Linux**: full bleed, no extra padding
- **macOS** (`icon.icns`): 10% transparent margin on each of the four sides

It generates `32x32.png`, `128x128.png`, `128x128@2x.png`, `icon.ico`, `icon.icns` and so on, and syncs them to `public/`.

The **Windows desktop icon** is written into the exe at Rust **link time** (from `icons/icon.ico`). After replacing `icon.png` you must run `npm run icons` first, then repackage.

If the installer or taskbar still shows the old icon:

1. Run a full package build: `npm run build:windows` (do not just run `cargo build`)
2. Install using **this** run's artifact: `target/release/bundle/msi/Pointer_*_x64_*.msi` (not a leftover old setup.exe under `bundle/nsis/`)
3. Uninstall the old version before installing the new one; Start-menu shortcuts may cache the icon, so you can delete the shortcut and reinstall
4. If it is still wrong, clean and rebuild: `cargo clean -p pointer-app`, then `npm run build:windows`

---

## GitHub Actions three-platform releases

Workflow: `.github/workflows/release.yml`

| Runner | Artifacts |
|--------|------|
| `windows-latest` | Windows MSI installer |
| `macos-latest` | Universal macOS (`--target universal-apple-darwin`) |
| `ubuntu-24.04` | Linux deb + AppImage |

**How to trigger:**

1. GitHub → Actions → **Release** → Run workflow  
2. Push a version tag:

```bash
git tag v0.1.0
git push origin v0.1.0
```

CI steps: checkout → Node 20 → Rust stable → (install Linux system dependencies) → `npm install` → `npm run icons` → `tauri-apps/tauri-action` → upload a **Draft Release**.

**This workflow produces standalone packages only**: all three targets carry `--config src-tauri/tauri.personal.conf.json`, no control-plane domains are injected, and there are no updater artifacts. Managed clients must be built locally or on enterprise CI — see [editions.md](../deploy/editions.md).

---

## Pre-build checklist

```bash
npm run build          # vue-tsc + vite build
cd src-tauri && cargo check && cd ..
npm run icons
npm run tauri:build
```

### Debug logging

```bash
# Bash / zsh / Linux
RUST_LOG=debug npm run tauri:dev

# PowerShell
$env:RUST_LOG="debug"; npm run tauri:dev
```

---

## Signing and distribution (official release)

The current configuration produces **unsigned** installers, which is enough for internal testing. For public releases we recommend:

| Platform | Recommendation |
|------|------|
| Windows | Authenticode code signing |
| macOS | Developer ID signing + Notarization |
| Linux | Decide per channel whether to GPG-sign the deb / verify the AppImage |

---

## Official domains and environment variables (desktop builds)

`npm run tauri:dev` / `tauri:build` load the local **`pointer.local.env`** (gitignored, see `pointer.local.env.example`); with no file present the control plane is unbound (purely local / standalone).  
To bind a control plane, set the domains in that file or in the environment plus `POINTER_EDITION=managed` (the word `official` only refers to Pointer's own releases; it is not a flavour value). `.github/workflows/release.yml` **produces standalone packages only** and injects no `POINTER_*` at all; managed (official / enterprise) packages are built locally or on enterprise CI. The full four-cell steps are in [editions.md](../deploy/editions.md).

---

## Related documents

| Document | Content |
|------|------|
| [editions.md](../deploy/editions.md) | **Packaging flavour × runtime form four cells**: build command / variables / artifacts / verification / external dependencies |
| [DEVELOPMENT.md](../DEVELOPMENT.md) | Day-to-day debugging, Skills, FAQ |
| [README.md](../../../README.md) | Project overview and quick start |
| [macos-computer-permissions.md](../../internals/macos-computer-permissions.md) | macOS computer-control permissions |

[Back to the guides index](README.md)
