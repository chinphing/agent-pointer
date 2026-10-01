# Standalone deployment developer documentation

English | [简体中文](../../zh-CN/developer/standalone-deployment.md)

pointer-server supports **being deployed standalone, detached from the official platform**. This document covers the architecture, configuration format, API endpoints and implementation modules.

## Table of contents

- [Deployment mode (deployment_mode.rs)](#deployment-mode)
- [Local authentication (local_auth.rs)](#local-authentication)
- [License system (license/)](#license-system)
- [Model configuration](#model-configuration)
- [API endpoints](#api-endpoints)
- [Configuration reference](#configuration-reference)
- [List of changed files](#list-of-changed-files)

---

## Deployment mode

**File:** `crates/pointer-core/src/deployment_mode.rs`


| Mode             | Description                                                                                 |
| -------------- | ---------------------------------------------------------------------------------- |
| `platform` (default when bound) | Connects to the control plane (the domain is pre-filled only for the **managed** flavour), OAuth + cloud keys |
| `standalone`   | Local username/password + model keys from the web settings. A **managed**-flavour standalone still validates the License; with no flavour set (self-built / unbound) it is not enforced |


**Balance / LLM gating:** the official account balance check (`ensure_llm_allowed`, `GET /auth/partner/llm-credentials`) takes effect only in `platform` mode; under `standalone` it is a no-op and the official balance API is not called.

```rust
pub fn deployment_mode() -> Mode;
pub fn is_standalone() -> bool;
```

Configuration source priority:

1. The `POINTER_DEPLOYMENT_MODE` environment variable (`platform` / `standalone`)
2. The `[deployment].mode` configuration file value
3. When neither is present, derive it from the control-plane binding: bound → `platform`, unbound → `standalone`

---



## Local authentication

**Files:** `crates/pointer-core/src/local_auth.rs`, `server/src/local_auth.rs`

In standalone mode this replaces the official OAuth. The configuration stores `username` + `password_hmac` (HMAC-SHA256 hex) + `hmac_secret`; no plaintext password is stored.

```rust
pub fn hmac_sha256_hex(secret: &str, password: &str) -> String;
pub fn verify_local_password(username: &str, password: &str) -> bool;
pub fn local_password_auth_configured() -> bool;
```

Operations generate a digest:

```bash
pointer-server --hash-password --secret '<hmac_secret>' '<password>'
```

**Flow:**

```
GET  /api/auth/mode              → { "mode": "standalone" | "platform" }
GET  /api/auth/local/captcha     → { captchaId, imageSvg }  (in-memory, single-use, TTL 5min)
POST /api/auth/local/login
  { username, password, captchaId, captcha }
  → verify captcha → verify_local_password
  → success: create a WebSession (auth_kind = Local) + Set-Cookie
  → failure: 401 invalid_captcha | invalid_credentials
```

**Session:** `WebSessionAuthKind::Local`; in standalone mode every API that requires login requires a local session.

The old `admin_token` / `POINTER_SERVER_ADMIN_TOKEN` is deprecated (warned about and ignored at startup).

### LLM credential path in standalone mode

In `chat_service/session_inner.rs`:

- standalone + web_session + `has_local_llm` → skip `ensure_llm_allowed()` (no platform quota check)
- standalone + no local LLM key → error telling you to configure an LLM

---



## License system

**Files:** `crates/pointer-core/src/license/mod.rs` + `verify.rs` + `fingerprint.rs`

### Ed25519 offline signing scheme

```
┌─────────────────────────────────┐
│          License Key            │
│  base64url(payload) . base64url(sig)  │
└─────────────────────────────────┘

payload (JSON):
{
  "customer_id": "acme",
  "expires_at": 1893455999,
  "features": ["chat", "webhook"],
  "max_seats": 50,
  "machine_id": "fp1:a1b2c3...",
  "machine_board_fp": "…",
  "machine_cloud_fp": "…"
}
```



### Module structure

```rust
pub struct LicenseClaims {
    pub customer_id, pub expires_at, pub features, pub max_seats,
    pub machine_id, pub machine_board_fp, pub machine_cloud_fp,
}
pub struct MachineFactors { pub os_id, pub board_uuid, pub cloud_provider, pub cloud_instance_id }
pub struct MachineFingerprints { pub strict, pub board, pub cloud }  // fp1:… + drift anchors

// core functions
pub fn validate_license_at_startup() -> Result<()>
pub fn reload_license_from_env() -> Result<LicenseStatusView>
pub fn current_machine_id() -> anyhow::Result<String>           // fp1:… binding token
pub fn current_machine_identity() -> Result<MachineIdentityView> // --machine-id-json
pub fn verify_machine_binding(...) -> Result<()>
```



### Verification flow

```
main()
  → deployment_mode == standalone?
    → read POINTER_LICENSE_KEY / [license].key
    → base64url-decode payload and signature
    → Ed25519 signature check (the public key is compiled in as license.pub; only debug builds allow POINTER_LICENSE_PUBLIC_KEY to override it, release ignores it)
    → check expiry
    → if claims.machine_id is non-empty, verify the v2 fingerprint or the legacy os id
    → v2: strict match, or the board/cloud drift anchors pass
    → cache claims → run
```

**Public key convention:** official release binaries only trust the compiled-in `license.pub`.
`POINTER_LICENSE_PUBLIC_KEY` can override it only in **debug** (`debug_assertions`), for local / e2e self-signing;
if a release build sets that variable it logs a warning and ignores it.


### Machine binding (v2 fingerprint)

`fingerprint.rs` collects multiple signals and produces a salted SHA-256:


| Signal         | Linux             | macOS          | Windows       | Cloud VM           |
| ---------- | ----------------- | -------------- | ------------- | -------------- |
| os_id      | `/etc/machine-id` | IOPlatformUUID | MachineGuid   | same as left             |
| board_uuid | DMI product_uuid  | IOPlatformUUID | WMI BIOS UUID | —              |
| cloud      | —                 | —              | —             | AWS/Azure IMDS |


- **strict** (`machine_id`): `fp1:` + SHA256(salt + os + board + cloud)
- **Drift anchors**: `machine_board_fp`, `machine_cloud_fp` — still verifiable after an OS reinstall changes strict
- **Legacy**: a bare os id string still matches exactly

```bash
./pointer-server --machine-id-json   # recommended for remote issuance
./pointer-server --machine-id        # fp1:… token only
```



### License generation CLI

**Files:** `tools/license-gen/`

```bash
# generate a key pair (keep the private key strictly offline, never in the repository)
cargo run -p pointer-license-gen -- gen-keypair \
  --private-key license.key \
  --public-key crates/pointer-core/license.pub

# sign (no machine binding)
cargo run -p pointer-license-gen -- sign \
  --private-key license.key \
  --customer-id acme \
  --expires 2027-12-31 \
  --features chat,webhook,channels

# sign (bind a specific machine JSON; recommended)
cargo run -p pointer-license-gen -- sign \
  --private-key license.key \
  --customer-id acme \
  --expires 2027-12-31 \
  --features chat,webhook \
  --machine-id-json ./identity.json

# sign (bind the current machine, including drift anchors)
cargo run -p pointer-license-gen -- sign \
  --private-key license.key \
  --customer-id acme \
  --expires 2027-12-31 \
  --features chat,webhook \
  --bind-machine

# sign (legacy bare os id, no drift)
cargo run -p pointer-license-gen -- sign \
  --private-key license.key \
  --customer-id acme \
  --expires 2027-12-31 \
  --machine-id "5A372B48-8721-5807-9645-8E7A560F2518"
```



### Hot reload (no restart)

```bash
# set a new license key
export POINTER_LICENSE_KEY="new_payload.new_signature"
# or edit pointer-server.toml [license].key
curl -X POST http://localhost:8787/api/license/reload
```

---



## Model configuration

Standalone does **not** read models, addresses or API keys from `pointer-server.toml`. The `[llm]` section in the configuration file is ignored.

After logging in, open **Settings → Model configuration → Custom services**, the same as custom services in the desktop client: add a provider (id, name, API address, model list, key), plus context, max output, thinking effort, capability checkboxes, `extra_body` and per-model overrides. Saving goes through `updateUserSettings` and writes `user_settings.json`; the key is stored encrypted on disk as `enc:v1:`.

No platform provider/model is built in locally any more (`activeProviderId` / `model` default to empty). Until you add a service in the UI and fill in a key, chat is unavailable.

---



## Platform side-effect management


| Module                                   | Standalone behaviour                                           |
| ------------------------------------ | ------------------------------------------------------- |
| `platform_endpoints.rs`              | Does not fall back to the official default domain (the source hardcodes none, so no injection means unbound); warns when unconfigured                       |
| `cloud_agent_auth.rs`                | Disables the cloud OAuth code exchange                                 |
| `token_usage_store.rs`               | `usage_report_enabled()` defaults to false, reporting is skipped                  |
| `media/oss.rs`                       | Reads from `[media_oss]` or `OSS_*` environment variables, not injected from OAuth            |
| `experiences.rs`                     | **Does not load** the official experience catalogue (returns empty directly; the welcome page shows no experience area)                    |
| `agents/computer/vision/annotate.rs` | Computer Agent annotation capability is degraded (requires a self-hosted `COMPUTER_ANNOTATE_API_BASE`) |


---



## API endpoints


| Endpoint                        | Method   | Description                                                         |
| ------------------------- | ---- | ---------------------------------------------------------- |
| `/api/auth/mode`          | GET  | `{ "mode": "standalone" | "platform" }`                    |
| `/api/auth/local/captcha` | GET  | `{ captchaId, imageSvg }`, standalone only                  |
| `/api/auth/local/login`   | POST | `{ username, password, captchaId, captcha }`, returns Set-Cookie |
| `/api/license/status`     | GET  | Returns license claims, status and machine binding information                                |
| `/api/license/reload`     | POST | Hot-reloads a new license from `POINTER_LICENSE_KEY`                       |




### License status response

```json
{
  "status": "valid",
  "customerId": "acme",
  "expiresAt": 1893455999,
  "features": ["chat", "webhook"],
  "maxSeats": 50,
  "machineBound": true,
  "currentMachineId": "5A372B48-8721-5807-9645-8E7A560F2518"
}
```

---



## Configuration reference

See `[server/pointer-server.toml.example](../../../server/pointer-server.toml.example)` for a complete example.

```toml
[deployment]
mode = "standalone"                    # platform (default) | standalone

[auth.local]
username = "admin"
hmac_secret = "replace-with-long-random-secret"
# pointer-server --hash-password --secret '<hmac_secret>' '<password>'
password_hmac = "...."

[license]
key = "base64_payload.base64_sig"      # license key string
# or: key_file = "license.key"         # read from a file

[usage]
report_enabled = false                 # standalone does not report usage by default

# models, keys and extra_body go in Web settings → Model configuration, not in this file.

[server]
addr = "0.0.0.0:8787"
public_url = "https://pointer.example.com"
app_data_dir = "/var/lib/pointer"
# zip: skills beside binary; deb:
# static_dir = "/usr/share/pointer-server/dist"
# skills_dir = "/usr/share/pointer-server/skills"
static_dir = "dist"
skills_dir = "skills"

# --- Web branding copy (optional; see docs/ui/web-branding-welcome-elapsed.md) ---
# page_title = "Acme · AI 助手"                 # env POINTER_SERVER_PAGE_TITLE
# composer_placeholder = "有什么可以帮你？"      # env POINTER_SERVER_COMPOSER_PLACEHOLDER
# welcome_tip_title = "我是 Acme AI 助手"        # env POINTER_SERVER_WELCOME_TIP_TITLE
# welcome_tip_body = "提交附件后我会自动处理…"    # env POINTER_SERVER_WELCOME_TIP_BODY
# turn_elapsed_active = "任务处理中"             # env POINTER_SERVER_TURN_ELAPSED_ACTIVE
# turn_elapsed_done = "任务已完成"               # env POINTER_SERVER_TURN_ELAPSED_DONE
# brand_name = "Acme 助手"                      # env POINTER_SERVER_BRAND_NAME
# brand_icon = "/branding/logo.png"            # env POINTER_SERVER_BRAND_ICON (shared by top-left / bottom-left)
# desktop_snapshot_enabled = false             # env POINTER_SERVER_DESKTOP_SNAPSHOT_ENABLED
# (the in-progress collapse bar requires "collapse execution process by default" to be enabled in assistant settings; the server does not force it)

# SSE first-frame padding comment frame (tunnels buffer-type firewalls; off by default)
# sse_padding_enabled = false   # env POINTER_SERVER_SSE_PADDING_ENABLED
# sse_padding_bytes = 10240    # env POINTER_SERVER_SSE_PADDING_BYTES

# Browser CORS (off by default, same-origin only). The desktop client goes through IPC and is unaffected.
# cors_origins = ["http://localhost:1420"]   # env POINTER_SERVER_CORS_ORIGINS
```

### `[server]` web branding / copy parameters

| TOML | Environment variable | Purpose | When unset |
|------|----------|------|----------|
| `page_title` | `POINTER_SERVER_PAGE_TITLE` | Browser tab `<title>` | `Pointer · AI 工作台` (Pointer · AI workspace) |
| `composer_placeholder` | `POINTER_SERVER_COMPOSER_PLACEHOLDER` | Input placeholder once logged in and usable | `告诉我你想做什么` (Tell me what you want to do) |
| `welcome_tip_title` | `POINTER_SERVER_WELCOME_TIP_TITLE` | Title of the welcome tip in a brand-new empty conversation | no tip is shown |
| `welcome_tip_body` | `POINTER_SERVER_WELCOME_TIP_BODY` | Body of the welcome tip in a brand-new empty conversation | no tip is shown |
| `turn_elapsed_active` | `POINTER_SERVER_TURN_ELAPSED_ACTIVE` | Prefix for the elapsed time of an in-progress turn | `工作` (Working) |
| `turn_elapsed_done` | `POINTER_SERVER_TURN_ELAPSED_DONE` | Prefix for the elapsed time of a finished turn | `工作` (Working) |
| `brand_name` | `POINTER_SERVER_BRAND_NAME` | Product name in the sidebar / top bar | `Pointer` |
| `brand_icon` | `POINTER_SERVER_BRAND_ICON` | Logo shared by the top-left and bottom-left corners | `/app-icon.png` |
| `desktop_snapshot_enabled` | `POINTER_SERVER_DESKTOP_SNAPSHOT_ENABLED` | Desktop screenshot button | auto-detect displays |
| `app_data_dir` | `POINTER_APP_DATA_DIR` | Runtime data root directory | OS default (the same rule as the desktop `PointerApp`) |
| `sse_padding_enabled` | `POINTER_SERVER_SSE_PADDING_ENABLED` | SSE first-frame padding | `false` |
| `sse_padding_bytes` | `POINTER_SERVER_SSE_PADDING_BYTES` | Padding byte count | `10240` |
| `cors_origins` | `POINTER_SERVER_CORS_ORIGINS` | Origins allowed for browser CORS | off (same-origin only) |

The "when unset" column quotes the built-in defaults verbatim — the parenthesised text translates the Chinese copy the product ships with. Behaviour and the frontend contract are in [`../ui/web-branding-welcome-elapsed.md`](../../zh-CN/ui/web-branding-welcome-elapsed.md). For local Vite development the same-named `VITE_*` variables override the meta (an empty `VITE_WEB_API_BASE` goes through the same-origin `/api` proxy, avoiding cross-site cookie loss).

### CORS

Off by default: no CORS layer is mounted, and only same-origin browser requests are served. The desktop client goes through Tauri IPC and is unaffected; when pointer-server also hosts the static UI it is same-origin too, so there is no need to enable it.

| Configuration | Behaviour |
|------|------|
| omitted / empty | CORS off |
| `cors_origins = ["*"]` or `POINTER_SERVER_CORS_ORIGINS=*` | Mirror any `Origin`, and allow cookies |
| `cors_origins = ["http://localhost:1420"]` | Exact allow-list (`localhost` and `127.0.0.1` are not the same Origin) |
| `"*"` mixed with concrete origins | Startup fails |

Local `web:dev` is same-origin by default (Vite proxies `/api` to 8787). If the frontend still talks to `http://127.0.0.1:8787` directly, CORS must be enabled. A split frontend/backend deployment (static pages and API on different origins) likewise needs the allowed origins configured.

---



## List of changed files



### New files (9)


| File                                           | Description                                          |
| -------------------------------------------- | ------------------------------------------- |
| `crates/pointer-core/src/deployment_mode.rs` | Deployment mode detection: `platform` / `standalone`            |
| `crates/pointer-core/src/local_auth.rs`      | Username/password HMAC local authentication                              |
| `crates/pointer-core/src/license/mod.rs`     | License module entry                                |
| `crates/pointer-core/src/license/verify.rs`  | Ed25519 verification, startup validation, hot reload, machine binding                    |
| `crates/pointer-core/license.pub`            | Public key file compiled in                                   |
| `server/src/local_auth.rs`                   | `/api/auth/local/login`, `/api/license/*` routes |
| `tools/license-gen/Cargo.toml`               | License issuance CLI                              |
| `tools/license-gen/src/main.rs`              | `gen-keypair` / `sign` subcommands                  |
| `tools/license-gen/dev-license.key.example`  | Example private key for development                                     |




### Changed files (12)


| File                                                      | Change                                                     |
| ------------------------------------------------------- | ------------------------------------------------------ |
| `Cargo.toml` (workspace)                                 | Add `tools/license-gen`                                 |
| `crates/pointer-core/Cargo.toml`                        | Add the `ed25519-dalek` dependency                                   |
| `crates/pointer-core/build.rs`                          | Compile in `license.pub`                                     |
| `crates/pointer-core/src/lib.rs`                        | Register the 3 new modules                                              |
| `crates/pointer-core/src/server_config.rs`              | Parse the `[deployment]`, `[auth.local]`, `[license]` sections |
| `crates/pointer-core/src/platform_endpoints.rs`         | standalone does not fall back to readflowai                              |
| `crates/pointer-core/src/web_request_auth.rs`           | `auth_kind` field                                         |
| `crates/pointer-core/src/cloud_agent_auth.rs`           | standalone disables cloud OAuth                                   |
| `crates/pointer-core/src/token_usage_store.rs`          | standalone does not report                                         |
| `crates/pointer-core/src/chat_service/session_inner.rs` | Local session skips the platform check                                      |
| `server/src/web_session.rs`                             | Session carries `auth_kind`                                 |
| `server/src/main.rs`                                    | Startup flow + `--machine-id` + routes + auth branching                      |


---



## Compilation and build



### Binary

```bash
cargo build -p pointer-server --release
```



### zip package (cross-platform, automatic)

```bash
npm run server:build
# macOS / Windows → zip
# Linux         → zip + .deb (when dpkg-deb is available)
```



### .deb package (Linux, already included in server:build)

Rebuilding the deb alone (an existing release binary is required):

```bash
npm run server:build:deb
# artifact: target/release/bundle/deb/pointer-server_0.1.0_*.deb
```



### Unit tests

```bash
cargo test -p pointer-core -- license::verify
```



### License issuance CLI

```bash
npm run license-gen:build
# artifact: target/release/license-gen-bundle/license-gen-{platform}-{arch}.zip
```

Development debugging:

```bash
npm run license-gen:dev -- sign --help
```

