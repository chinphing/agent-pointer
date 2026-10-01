# Platform packaging index (Windows / macOS / Linux)

English | [简体中文](../../zh-CN/deploy/platforms.md)

**This page is an index only**: for each of the three platforms it lists the **prerequisites / build command / artifact location / CI policy**, and every section links to the full text in [`../contributing/cross-platform-build.md`](../contributing/cross-platform-build.md). The body is not duplicated here (that document is cross-referenced widely, so stable anchors win).

> The four-cell packaging-flavour × runtime-form steps are in [`editions.md`](editions.md); the main entry is [`README.md`](README.md).

---

## 1. Prerequisites

| Platform | Extra requirements | Full text |
|---|---|---|
| **Windows** | **Microsoft C++ Build Tools** (select "Desktop development with C++"), **WebView2 Runtime**, **LLVM / libclang** (`silk-v3-sys` goes through bindgen; needed only when **building from source** — end users installing the MSI do not need it) | [Windows › Requirements](../contributing/cross-platform-build.md#windows) |
| **macOS** | `xcode-select --install`; building the `.app` / `.dmg` **must happen on macOS** | [macOS › Requirements](../contributing/cross-platform-build.md#macos) |
| **Linux (Ubuntu / Debian)** | Build baseline **ubuntu-24.04** (PipeWire ≥ 1.0 headers; artifacts need glibc ≥ 2.39); one-shot dependency install: `bash scripts/install-linux-build-deps.sh` | [Linux › System dependencies](../contributing/cross-platform-build.md#linux) |

Prerequisites common to all three platforms: **Node.js 20+**, **Rust stable**, and a local C compiler (to build the `sqlite-cjk-fts` native extension) — see [Common dependencies](../contributing/cross-platform-build.md#common-dependencies).

---

## 2. Client (Tauri desktop app)

| Platform | Command | Artifacts |
|---|---|---|
| Windows | `npm run icons && npm run build:windows` | `src-tauri/target/release/bundle/msi/*.msi`; `src-tauri/target/release/pointer-app.exe` |
| macOS | `npm run icons && npm run build:macos` (Universal: `build:macos:universal`; signed: `build:macos:signed`) | `src-tauri/target/release/bundle/{macos,dmg}/**`; Universal lands in `src-tauri/target/universal-apple-darwin/release/bundle/**` |
| Linux | `npm run icons && npm run build:linux` | `src-tauri/target/release/bundle/{deb,appimage}/**` |

Full commands, format trade-offs and troubleshooting per platform are in the matching sections: [Windows](../contributing/cross-platform-build.md#windows) · [macOS](../contributing/cross-platform-build.md#macos) · [Linux](../contributing/cross-platform-build.md#linux). Signing and distribution (Authenticode / Developer ID + notarisation / GPG) are in [Signing and distribution (official releases)](../contributing/cross-platform-build.md#signing-and-distribution-official-release).

---

## 3. Server (`pointer-server`)

| Item | Content |
|---|---|
| Command | `npm install && npm run server:build` |
| Artifacts | `target/release/pointer-server-bundle/pointer-server-{platform}-{arch}.zip` (Linux additionally ships a `.deb`) |
| Full text | [Web development and deployment](../contributing/cross-platform-build.md#web-development-and-deployment) · [Common packaging flow](../contributing/cross-platform-build.md#common-packaging-flow) |

The server is not constrained by platform packaging formats: **the zip works everywhere**, and the `.deb` is attached automatically by the Linux build. For the four flavours, variables and runtime configuration see [`editions.md`](editions.md).

---

## 4. CI policy

`.github/workflows/release.yml` **produces standalone client packages only**: all three targets carry `--config src-tauri/tauri.personal.conf.json`, no control-plane domains are injected, and there are no updater artifacts. **Managed clients must be built locally or on enterprise CI** — see [`editions.md`](editions.md).

Detailed steps (how to trigger, Linux system dependencies, Draft Release) are in [GitHub Actions three-platform releases](../contributing/cross-platform-build.md#github-actions-three-platform-releases).

---

## 5. Related documents

| Document | Content |
|---|---|
| [`README.md`](README.md) | Packaging and deployment four-cell checklist entry |
| [`editions.md`](editions.md) | Flavour × runtime-form four cells (build command / variables / artifacts / verification / external dependencies) |
| [`../contributing/cross-platform-build.md`](../contributing/cross-platform-build.md) | Platform environments, local development, build/packaging and CI full text |
| [`../internals/standalone-server-deployment.md`](../internals/standalone-server-deployment.md) | Full standalone server delivery flow |

[Back to the documentation index](../README.md)
