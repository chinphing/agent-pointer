# Packaging and deployment: the four-cell checklist entry

English | [简体中文](../../zh-CN/deploy/README.md)

One source tree; three orthogonal axes decide what you need to do:

| Axis | Values | Where |
|---|---|---|
| **Packaging flavour** `POINTER_EDITION` | `managed` (centrally managed, control-plane domains injected at build time) / **unset** (standalone, independent) | §1–§4 (the four-cell checklist: managed/standalone × client/server) below |
| **Runtime form** | client (Tauri desktop app) / server (`pointer-server`) | §1–§4 (the four-cell checklist: managed/standalone × client/server) below |
| **Target platform** | Windows / macOS / Linux | [§5 Platform packaging](#platforms) |

**This page is the single entry point.** First locate your cell in §0 ("Locate yourself first") and work through its checklist; go to the detailed manuals only when you need the detail.

> **Terminology**: `official` is **not** a `POINTER_EDITION` value; it only refers to a Pointer official release.
> Writing any other value (including `official`) makes the build **fail outright** — deliberately so, to avoid silently degrading into an unbound build.

---

<a id="where"></a>
## 0. Locate yourself first: what are you trying to do?

| What you want to do | Your cell | Where |
|---|---|---|
| Use it yourself, or hand a **self-built** client to a colleague | standalone × client | [§3 standalone × client](#standalone-client) |
| Stand up a server for your team | standalone × server | [§4 standalone × server](#standalone-server) |
| Enterprise: client connecting to an **internal** control plane | managed × client | [§1 managed × client](#managed-client) |
| Enterprise: deploy the internal control-plane server | managed × server | [§2 managed × server](#managed-server) |
| You want a Pointer **official signed package** | —— | Not produced by this repo; see [`../user/which-build.md`](../user/which-build.md) |

**Master table** (five items per cell: build command / required variables / artifact location / how to verify / external dependencies):

| Flavour | Client (Tauri desktop app) | Server (`pointer-server`) |
|------|--------------------------|-----------------------------|
| **`managed`** (centrally managed) | [§1 managed × client](#managed-client) | [§2 managed × server](#managed-server) |
| **standalone** (independent, default) | [§3 standalone × client](#standalone-client) | [§4 standalone × server](#standalone-server) |

Detailed version (artifact lists, troubleshooting, CI policy): [`editions.md`](editions.md).

---

<a id="managed-client"></a>
## 1. managed × client

- [ ] **1. Prepare variables** — in `pointer.local.env` write: `POINTER_EDITION=managed`, `VITE_POINTER_EDITION=managed`, the three domains (`POINTER_API_BASE` / `POINTER_WEB_BASE` / `COMPUTER_ANNOTATE_API_BASE`), and the updater signing private key (`TAURI_SIGNING_PRIVATE_KEY` or `TAURI_SIGNING_PRIVATE_KEY_PATH`)
- [ ] **2. Install dependencies and icons** — `npm install && npm run icons`
- [ ] **3. Build** — `npm run tauri:build`
- [ ] **4. Verify** — the build log contains `[tauri-build] POINTER_EDITION=managed`; `find src-tauri/target/release/bundle -name '*.sig'` returns results; after launch the About page shows the update entry
- [ ] **5. Distribute** — artifacts under `src-tauri/target/release/bundle/**`

**External dependencies**: the three control-plane domains reachable (internal domains for enterprises); the updater signing private key.

**Why the signing key is mandatory**: `bundle.createUpdaterArtifacts` in `src-tauri/tauri.conf.json` is `true` (only the standalone `tauri.personal.conf.json` turns it off), so a missing key fails at the signing stage.

---

<a id="managed-server"></a>
## 2. managed × server

- [ ] **1. Prepare variables** — `POINTER_EDITION=managed` + `VITE_POINTER_EDITION=managed` + the three domains. Write them into `pointer.local.env` or export them explicitly (**OS / CI environment variables win**; the file only fills gaps)
- [ ] **2. Build** — `npm install && npm run server:build`
- [ ] **3. Verify the domains are baked in** — the build log contains `[server-build] edition=managed domains=3/3 baked, web=managed`; `strings target/release/pointer-server | grep -m1 <your api_base domain>` prints output
- [ ] **4. Runtime configuration** — `[pointer]` in `pointer-server.toml`: `api_base`, `oauth_client_secret` (aligned with the control plane's `THIRD_PARTY_OAUTH_EXCHANGE_SECRET`), `public_url`; the callback URL is already registered on the control plane
- [ ] **5. Start and verify** — the log shows `deployment_mode: platform (control_plane_bound=true)`; `curl -s http://127.0.0.1:8787/api/auth/mode` → `{"mode":"platform"}`; opening `public_url` in a browser redirects to control-plane OAuth login
- [ ] **6. Remember** — platform mode **skips license verification**; no `[license]` needed

**External dependencies**: the control plane (Pointer's site or your own) API reachable, the OAuth secret aligned, the callback URL registered.

---

<a id="standalone-client"></a>
## 3. standalone × client

- [ ] **1. Variables** — **none**. Make sure there is no `pointer.local.env`, or that it carries no flavour and no domains
- [ ] **2. Install dependencies and icons** — `npm install && npm run icons`
- [ ] **3. Build** — `npm run tauri:build` (the script appends `--config src-tauri/tauri.personal.conf.json` automatically, turning off updater artifacts)
- [ ] **4. Verify** — the build log contains `[tauri-build] standalone build (no updater artifacts)`; there is **no** `*.sig` in the bundle; the startup log contains `edition: unset (standalone defaults)`; the bottom-left corner shows "Local mode"

**External dependencies**: none. Fill in your own model API key after installing and you are ready to go.

---

<a id="standalone-server"></a>
## 4. standalone × server

- [ ] **1. Build** — `npm install && npm run server:build`
- [ ] **2. Configure** — `cp server/pointer-server.toml.example pointer-server.toml`, set `[deployment] mode = "standalone"` and the credentials
- [ ] **3. Start and verify** — `curl -s http://127.0.0.1:8787/api/auth/mode` → `{"mode":"standalone"}`; browser login with the credentials succeeds; sending a message gets a reply (first fill in the API key under Settings → Models)
- [ ] **4. License** — **not enforced**: with no flavour set it runs as `notConfigured` and licensed features stay off

**External dependencies**: none.

**Full delivery flow** (build → license issuance → customer rollout → acceptance → troubleshooting) is in [`../internals/standalone-server-deployment.md`](../internals/standalone-server-deployment.md); customer-side operations are in [`../user/standalone-server.md`](../user/standalone-server.md).

---

<a id="platforms"></a>
## 5. Platform packaging

Environment prerequisites, build commands, artifact locations and CI policy for the three platforms are in [`platforms.md`](platforms.md) (full text in [`../contributing/cross-platform-build.md`](../contributing/cross-platform-build.md)).

---

## 6. Related documents

| Document | Content |
|---|---|
| [`editions.md`](editions.md) | The four cells in detail (build command / variables / artifacts / verification / external dependencies) |
| [`platforms.md`](platforms.md) | Windows / macOS / Linux platform packaging index (prerequisites / artifacts / CI policy) |
| [`../contributing/cross-platform-build.md`](../contributing/cross-platform-build.md) | Windows / macOS / Linux environments and packaging commands |
| [`../internals/standalone-server-deployment.md`](../internals/standalone-server-deployment.md) | Full standalone server delivery flow (build → license → rollout → acceptance) |
| [`../developer/standalone-deployment.md`](../developer/standalone-deployment.md) | pointer-server configuration reference (implementation-oriented) |
| [`../user/which-build.md`](../user/which-build.md) | How official signed packages differ from local builds (user view) |
| [`../user/standalone-server.md`](../user/standalone-server.md) | Running your own pointer-server |
| [`../design/control-plane-and-editions.md`](../design/control-plane-and-editions.md) | Account and control-plane rework design (background) |

[Back to the documentation index](../README.md)
