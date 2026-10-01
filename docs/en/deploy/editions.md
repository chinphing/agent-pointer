# Packaging flavour × runtime form: the four-cell build and deployment

English | [简体中文](../../zh-CN/deploy/editions.md)

One source tree, where the **packaging flavour** (`POINTER_EDITION`) and the **runtime form** (client / server) are orthogonal. This document gives, for each of the four combinations, the **build command / required variables / artifact location / how to verify / external dependencies**, so a packaging maintainer can simply follow along.

> The account and control-plane rework design (P0/P1) is in [`../design/control-plane-and-editions.md`](../design/control-plane-and-editions.md); cross-platform environment preparation is in [`../contributing/cross-platform-build.md`](../contributing/cross-platform-build.md).

---

## 0. Terminology

| Term | Meaning |
| --- | --- |
| Flavour **`managed` (centrally managed)** | Control-plane domains injected at build time: the build talks to the control plane by default (login, catalogue, usage, auto-update). |
| Flavour **standalone (independent)** | `POINTER_EDITION` is **unset**. The source writes no control-plane domain ⇒ unbound: local models / keys; the server uses credentials or `?sso=`. |
| **Client** | The Tauri desktop app (`src-tauri/`). The agent runs in a local process and does not depend on pointer-server. |
| **Server** | `pointer-server` (axum HTTP/SSE + the same Vue interface). For browsers / cloud hosts. |

The four cells:

| | Client (Tauri) | Server (pointer-server) |
| --- | --- | --- |
| **managed** | Official client / enterprise internal client | Official cloud-host image / enterprise control-plane server |
| **standalone** | Local client (default) | Self-hosted server (credential login) |

---

## 1. Flavour and binding (shared premise of all four cells)

The flavour only decides the **build-time defaults**; it is not a runtime switch. Whether the control plane is reachable at runtime is decided by `platform_endpoints::control_plane_bound()` / `deployment_mode::is_standalone()` (see the module comment in `crates/pointer-core/src/edition.rs`).

| Variable | Purpose |
| --- | --- |
| `POINTER_EDITION=managed` | Packaging flavour, baked in at compile time. **When control-plane domains are missing, `crates/pointer-core/build.rs` panics outright and the build fails** |
| `VITE_POINTER_EDITION=managed` | Frontend flavour (`src/lib/platformUrls.ts` → `isManagedEdition()`) |
| `POINTER_API_BASE` | Control-plane API domain (baked into `POINTER_BUILTIN_API_BASE` at build time) |
| `POINTER_WEB_BASE` / `VITE_POINTER_WEB_BASE` | Control-plane Web domain (top-up / cloud-host redirects) |
| `COMPUTER_ANNOTATE_API_BASE` | SOM annotation service domain (baked in at build time) |
| `POINTER_DOWNLOAD_URL` / `VITE_POINTER_DOWNLOAD_URL` | Download page link (About page) |
| `TAURI_SIGNING_PRIVATE_KEY` / `TAURI_SIGNING_PRIVATE_KEY_PATH` | Updater signing private key, **needed only by the managed client** |

With no flavour set, the source writes no control-plane default ⇒ unbound ⇒ standalone.

**Load order (easy to trip over)**

- `npm run tauri:dev` / `tauri:build` / Vite (`web:dev`) automatically read the gitignored `pointer.local.env` at the repository root (**pre-existing OS / CI environment variables win**; the file only fills the blanks). Template: [`pointer.local.env.example`](../../../pointer.local.env.example).
- `npm run server:build` (`scripts/build-server.mjs`) **reads it too**, and passes the same set of variables through to both the Vue build and `cargo` (neither side reads its own copy). After the build and before packaging it also verifies that the two halves agree: under the managed flavour, a binary missing control-plane domains or web assets missing the web base aborts with a non-zero exit (see §4.2).
- At runtime the same-named `POINTER_*` environment variables can still override the build-time defaults; the server additionally has `POINTER_DEPLOYMENT_MODE`.

---

## 2. The four cells at a glance

| Cell | Build command | Key variables | Artifacts | One-line verification | External dependencies |
| --- | --- | --- | --- | --- | --- |
| [managed × client](#managed-client) | `npm run tauri:build` (or `build:windows` / `build:macos` / `build:linux`) | `POINTER_EDITION=managed` + 3 domains + updater private key | `src-tauri/target/release/bundle/**` + `*.sig` | build log `[tauri-build] POINTER_EDITION=managed`; `*.sig` present in the bundle | control-plane domains, updater private key |
| [managed × server](#managed-server) | `npm run server:build` (write `managed` + 3 domains into `pointer.local.env`, or export them) | same as above (without the updater key) + `[pointer]` runtime configuration | `target/release/pointer-server-bundle/pointer-server-{platform}-{arch}.zip` (Linux additionally `.deb`) | build log `[server-build] edition=managed domains=3/3 baked, web=managed`; startup log `deployment_mode: platform (control_plane_bound=true)` | control-plane OAuth (secret aligned with the control plane) |
| [standalone × client](#standalone-client) | `npm run tauri:build` | none (no flavour set) | same directory, **without** `*.sig` | build log `[tauri-build] standalone build (no updater artifacts)` | none |
| [standalone × server](#standalone-server) | `npm run server:build` | `[deployment] mode = "standalone"` + credentials | same zip / deb | `/api/auth/mode` returns `standalone`, credential login succeeds | none (official signed packages need a license) |

---

<a id="managed-client"></a>
## 3. managed × client

### 3.1 Build command

```bash
cd agent-pointer
npm install
npm run icons                     # first time, or after replacing icon.png

# Option A: write managed + domains into the local pointer.local.env (everyday)
npm run tauri:build

# Option B: export for one run (CI / temporary; key names match pointer.local.env.example)
POINTER_EDITION=managed VITE_POINTER_EDITION=managed \
  POINTER_API_BASE=https://pointer-api.example.com \
  POINTER_WEB_BASE=https://pointer.example.com \
  COMPUTER_ANNOTATE_API_BASE=https://pointer-som.example.com \
  VITE_POINTER_WEB_BASE=https://pointer.example.com \
  POINTER_DOWNLOAD_URL=https://pointer.example.com/download \
  TAURI_SIGNING_PRIVATE_KEY_PATH=~/.tauri/pointer-updater.key \
  npm run tauri:build
```

Platform shortcut scripts (all going through the same `scripts/tauri-build.mjs`): `build:windows` / `build:macos` (signed Universal build `build:macos:signed`) / `build:linux`.

Generating the updater key for the first time (do it once, keep the private key offline):

```bash
npm run tauri signer generate -w ~/.tauri/pointer-updater.key
```

`bundle.createUpdaterArtifacts` in `src-tauri/tauri.conf.json` is `true` (only the standalone `tauri.personal.conf.json` turns it off), so the managed client **must** provide a signing private key, otherwise the updater artifact signing stage fails. `scripts/tauri-build.mjs` reads the contents of `TAURI_SIGNING_PRIVATE_KEY_PATH` into `TAURI_SIGNING_PRIVATE_KEY`.

### 3.2 Required variables

| Variable | Required | Notes |
| --- | --- | --- |
| `POINTER_EDITION=managed` | ✅ | Flavour. **Only `managed` or empty is accepted**: any other value (including the old `official`) makes the build fail outright, avoiding a silent downgrade to unbound |
| `VITE_POINTER_EDITION=managed` | ✅ | Frontend flavour; when unset the script fills it in from `POINTER_EDITION` |
| `POINTER_API_BASE` / `POINTER_WEB_BASE` / `COMPUTER_ANNOTATE_API_BASE` | ✅ | The three control-plane domains; missing any one panics the build |
| `VITE_POINTER_WEB_BASE` / `POINTER_DOWNLOAD_URL` | Recommended | Frontend redirects and the About page download link |
| `TAURI_SIGNING_PRIVATE_KEY` or `…_PATH` | ✅ | Updater signing private key |

### 3.3 Artifact location

```text
src-tauri/target/release/bundle/
├── msi/  nsis/          # Windows installers
├── macos/  dmg/         # macOS (.app / .dmg)
├── deb/  appimage/      # Linux
└── *.sig + updater archives (Windows `.nsis.zip`, macOS `.app.tar.gz`, Linux `.AppImage.tar.gz`)
```

- Signed Universal build artifacts land in `target/universal-apple-darwin/release/bundle/`.
- Find updater artifacts: `find src-tauri/target/release/bundle -name '*.sig'` (whatever the actual artifacts are).

### 3.4 How to verify

1. The build log shows `[tauri-build] POINTER_EDITION=managed`.
2. `find src-tauri/target/release/bundle -name '*.sig'` is non-empty (the updater artifacts are signed).
3. The domains are baked in: `strings <release binary> | grep -m1 <your api_base domain>` (after a macOS build this is `Pointer.app/Contents/MacOS/Pointer`).
4. Start the app with `RUST_LOG=info`; the log shows `edition: managed`; sending a message while signed out reports "Sign in to your Pointer account" (bound to a control plane with no identity).
5. The About page "Download" link points at `POINTER_DOWNLOAD_URL`.

### 3.5 External dependencies

- The control-plane API / Web / SOM domains reachable (internal domains for enterprises).
- The updater private key (the public key already lives in `src-tauri/tauri.conf.json` → `plugins.updater.pubkey`; **rotating the key means updating this too**).
- Release: the installer + updater archive + `.sig` go through the console's client-release flow.

---

<a id="managed-server"></a>
## 4. managed × server

> Current state: `scripts/build-server.mjs` reads `pointer.local.env` (pre-existing OS / CI variables win) and passes the same environment to both the Vue build and `cargo`; `crates/pointer-core/build.rs` bakes the domains into the binary at compile time. After the build and before packaging, `scripts/lib/verify-baked-edition.mjs` verifies that the two halves agree: under the managed flavour, a missing half fails the build.

### 4.1 Build command

```bash
cd agent-pointer
npm install

# Option A: write managed + the 3 domains into the local pointer.local.env (everyday)
npm run server:build

# Option B: export for one run (CI / temporary; key names match pointer.local.env.example)
POINTER_EDITION=managed \
VITE_POINTER_EDITION=managed \
POINTER_API_BASE=https://pointer-api.example.com \
POINTER_WEB_BASE=https://pointer.example.com \
COMPUTER_ANNOTATE_API_BASE=https://pointer-som.example.com \
VITE_POINTER_WEB_BASE=https://pointer.example.com \
npm run server:build
```

`server:build` = same-origin Vue build (`VITE_WEB_API_BASE` emptied) → `cargo build -p pointer-server --release` → **verify that both halves share the flavour** → zip; on Linux, add a `.deb` when `dpkg-deb` is available. `--package-only` / `--deb-only` skip recompiling but still verify the existing artifacts.

### 4.2 Required variables

**Build time** (the 3 domains + flavour from the table above; write them into `pointer.local.env` or export them):

| Variable | Required | Notes |
| --- | --- | --- |
| `POINTER_EDITION=managed` | ✅ | Flavour; also the switch that panics when domains are missing |
| `VITE_POINTER_EDITION=managed` | Recommended | Frontend flavour (visibility of the cloud-host / top-up entries) |
| `POINTER_API_BASE` / `POINTER_WEB_BASE` / `COMPUTER_ANNOTATE_API_BASE` | ✅ | The three control-plane domains |
| `VITE_POINTER_WEB_BASE` | Recommended | Frontend redirect to the control-plane Web |

**Runtime** (`pointer-server.toml`, see `server/pointer-server.toml.example`):

| Setting | Notes |
| --- | --- |
| `[pointer].api_base` | Control-plane API (the `POINTER_API_BASE` environment variable wins). Left empty it falls back to the build-time baked value |
| `[pointer].oauth_client_secret` | Must match the control plane's `THIRD_PARTY_OAUTH_EXCHANGE_SECRET` (**not** JWT_SECRET) |
| `[server].public_url` | Browser-reachable root URL; the OAuth callback `{public_url}/api/auth/oauth/callback` must be registered in the control plane's `POINTER_APP_REDIRECT_URIS` |
| `[deployment].mode` | Leaving it empty is fine: a bound control plane ⇒ `platform` automatically; `platform` can also be written explicitly |
| `[license]` | **Not needed**: platform mode skips license verification (log `license: skipped (platform deployment mode)`) |

### 4.3 Artifact location

```text
target/release/pointer-server-bundle/pointer-server-{macos-arm64|macos-x64|windows-x64|linux-x64}.zip
target/release/bundle/deb/pointer-server_0.1.0_{amd64|arm64}.deb      # Linux
```

For the package layout (binary + `dist/` + `skills/` + `pointer-server.toml.example` + start/stop scripts) see section 2 of [`../internals/standalone-server-deployment.md`](../internals/standalone-server-deployment.md).

### 4.4 How to verify

```bash
# 0) Read the guard verdict in the build log first (packaging continues only when both halves agree)
#    [server-build] edition=managed domains=3/3 baked, web=managed

# 1) The domains are baked in
strings target/release/pointer-server | grep -m1 <your api_base domain>

# 2) Start (first copy pointer-server.toml.example to pointer-server.toml and fill in [pointer])
./target/release/pointer-server
#    expected log: deployment_mode: platform (control_plane_bound=true)

# 3) Authentication mode (no longer local credentials)
curl -s http://127.0.0.1:8787/api/auth/mode      # → {"mode":"platform"}

# 4) Open public_url in a browser → redirects to control-plane OAuth login
```

### 4.5 External dependencies

- The control plane (Pointer's site or a self-hosted control plane) API reachable, the OAuth secret aligned, the callback URL registered.
- Control-plane-side organisation / billing / shop (not built by this repository).
- No license issuance needed.

---

<a id="standalone-client"></a>
## 5. standalone × client

### 5.1 Build command

```bash
cd agent-pointer
npm install
npm run icons
npm run tauri:build          # with no POINTER_* variable set
```

Having no `pointer.local.env` (or a file without domains / flavour) means this flavour. `scripts/tauri-build.mjs` appends `--config src-tauri/tauri.personal.conf.json` automatically (turning off updater artifacts).

### 5.2 Required variables

No required variables. Optional: `POINTER_USAGE_REPORT_ENABLED=false` (already off by default when unbound).

### 5.3 Artifact location

`src-tauri/target/release/bundle/{msi,nsis,macos,dmg,deb,appimage}`, **without** `*.sig` / updater archives.

### 5.4 How to verify

1. The build log shows `[tauri-build] standalone build (no updater artifacts)`.
2. `find src-tauri/target/release/bundle -name '*.sig'` is empty.
3. After launch without signing in: fill in the API key under Settings → Models and you can chat; the cloud host page opens but cannot be purchased from.
4. With `RUST_LOG=info` the log shows `edition: unset (standalone defaults)`.

### 5.5 External dependencies

None.

---

<a id="standalone-server"></a>
## 6. standalone × server

### 6.1 Build command

```bash
cd agent-pointer
npm install
npm run server:build
```

The full delivery flow (build → license issuance → customer rollout → acceptance → troubleshooting) is in [`../internals/standalone-server-deployment.md`](../internals/standalone-server-deployment.md); configuration and implementation reference in [`../developer/standalone-deployment.md`](../developer/standalone-deployment.md).

### 6.2 Required variables

| Setting | Notes |
| --- | --- |
| `[deployment] mode = "standalone"` (or `POINTER_DEPLOYMENT_MODE=standalone`) | already baked into the deb's systemd unit |
| `[auth.local]` | `username` / `hmac_secret` / `password_hmac` (generate with `pointer-server --hash-password`) |
| `[license].key` | **mandatory only for a managed-flavour standalone server** (see below) |
| `[server].public_url` | the HTTPS domain behind the reverse proxy; affects the OAuth callback and IM large-file download links |
| `[usage] report_enabled` | defaults to `false` |

License rules (`crates/pointer-core/src/license/verify.rs`):

| Combination | Startup verification |
| --- | --- |
| managed + standalone | **mandatory**: missing / expired / bound to the wrong machine fails startup outright |
| managed + platform | skipped (`license: skipped (platform deployment mode)`) |
| no flavour set (self-hosted) | not enforced: runs as `notConfigured`, licensed features off |

### 6.3 Artifact location

Same as [4.3](#managed-server): `target/release/pointer-server-bundle/pointer-server-{platform}-{arch}.zip` (Linux additionally `.deb`).

### 6.4 How to verify

```bash
curl -s http://127.0.0.1:8787/api/auth/mode       # → {"mode":"standalone"}
curl -s http://127.0.0.1:8787/api/license/status  # self-hosted notConfigured; official signed package valid
```

Open `public_url` in a browser → sign in with account, password and captcha → sending a message gets a reply (first fill in the API key under Settings → Models). The full acceptance checklist is in section 8 of the internals document.

### 6.5 External dependencies

None. An official signed package needs a license issued by Pointer (`npm run license-gen:build`).

---

## 7. CI and release policy

- `.github/workflows/release.yml` **produces standalone client packages only**: all three targets carry `--config src-tauri/tauri.personal.conf.json`, inject no `POINTER_*` at all, and upload the artifacts to a Draft Release. **managed (official / enterprise) clients are built locally or on enterprise CI**.
- There is no CI for the managed server: it must be built on a machine / pipeline carrying the control-plane variables (copy [4.1](#managed-server)). `scripts/build-server.mjs` hands `pointer.local.env` or the CI environment variables to both halves and verifies before packaging; on CI use environment variables (there is no `pointer.local.env` file).
- After releasing a managed client, the installer + updater archive + `.sig` go through the console's client-release flow; standalone packages go straight to a GitHub Release.

---

## 8. Everyday development

```bash
cp pointer.local.env.example pointer.local.env    # create it only when you need to bind a control plane
npm run tauri:dev                                 # reads pointer.local.env automatically; no file ⇒ standalone
npm run server:dev                                # server debugging (unaffected by pointer.local.env)
```

| Role | Recommendation |
| --- | --- |
| Open source / individual | do not create the file (no domains set ⇒ unbound ⇒ standalone) |
| Official-site maintainer | write `managed` + the official API/Web/SOM domains into `pointer.local.env` |
| Enterprise internal | also write `managed`, with the domains pointing at the internal control plane |

Never write production domains or signing passphrases into the public `main`. When you need to work against a control plane, change only the local `pointer.local.env`.

Loading implementation: `scripts/lib/load-pointer-local-env.mjs` (called by `tauri-dev.mjs` / `tauri-build.mjs` / `build-server.mjs` / `vite.config.ts`). `POINTER_*` and the matching `VITE_*` fill in each other's blanks. Server two-halves consistency check: `scripts/lib/verify-baked-edition.mjs` (called by `build-server.mjs` before packaging).

---

## 9. Maintenance flow

- General features: a PR to the public repository → the local package and the control-plane-bound package share it.
- Paid cloud capabilities only: change the private cloud / billing / update service.
- Rotating domains, updating the public key or certificates: change only the official CI or the maintainer's local `pointer.local.env`; never commit it.
- External PRs are assumed unbound by default; before merging, confirm no single control-plane domain has been hardcoded.

---

## 10. Pre-release checklist

- Signing keys live only in CI secrets / environments; never in the repository, never in a local config file.
- Turn on Private vulnerability reporting on GitHub.
- Before releasing a managed package, confirm: `*.sig` complete, domains pointing at the right environment (production / staging not mixed up), `plugins.updater.pubkey` paired with the signing private key.
- Before releasing a managed server, confirm: the `npm run server:build` log reads `edition=managed domains=3/3 baked, web=managed` (a failing guard aborts packaging outright).
