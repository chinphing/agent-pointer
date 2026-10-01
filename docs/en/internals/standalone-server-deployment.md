# pointer-server standalone deployment: the full flow (internal)

English | [简体中文](../../internals/standalone-server-deployment.md)

For the Pointer team: a one-stop operations manual covering **building the release package, issuing a License, and bringing a customer-side standalone deployment online**.

For the user-facing guide read [`../user/standalone-server.md`](../user/standalone-server.md); for implementation details see [`../developer/standalone-deployment.md`](../developer/standalone-deployment.md).

---

## 1. Roles and deliverables

| Role | Responsibility | Deliverable |
|------|------|--------|
| **Pointer build machine** | Compiles server / license-gen | zip or deb installer |
| **Pointer issuer** | Safekeeps the Ed25519 private key, signs Licenses for customers | `license.key` (offline), the issued License string |
| **Customer operations** | Deploys the server, configures the LLM, account and password | A reachable Web UI + API |

Key points of standalone mode:

- **A standalone package of the managed flavour must** carry a valid License (Ed25519 signature verification; the public key is compiled into `crates/pointer-core/license.pub`); with no flavour set (self-hosted / unbound) it is not enforced — without one it runs as `notConfigured` and licensed features are off; platform mode (bound control plane) skips License verification
- You **must** add a provider and fill in the API Key under Web Settings → Models (do not write it in the TOML)
- **Sign in with account, password and captcha**; readflowai.com OAuth is not used
- By default Token usage is **not reported** (`report_enabled = false`)

---

## 2. Building the release package (Pointer side)

This section builds the **standalone (independent)** package. A **managed (centrally managed)** server additionally needs control-plane domains injected (`scripts/build-server.mjs` reads `pointer.local.env`; OS / CI environment variables may also be exported explicitly; before packaging it verifies that the two halves agree), see [`../deploy/editions.md`](../deploy/editions.md#managed-server).

The unified commands (the same `{module}:dev|build` style as the desktop `tauri:build`):

```bash
# Development / debugging
npm run server:dev

# Packaging (the artifact is selected automatically for the current OS)
npm run server:build
```

| Build machine OS | Artifact |
|-----------|------|
| macOS | `target/release/pointer-server-bundle/pointer-server-macos-{arm64\|x64}.zip` |
| Windows | `target/release/pointer-server-bundle/pointer-server-windows-x64.zip` |
| Linux | the zip above + `target/release/bundle/deb/pointer-server_0.1.0_{amd64\|arm64}.deb` (needs `dpkg-deb`) |

Directory layout inside the zip:

```text
pointer-server/
├── pointer-server              # Linux/macOS binary (pointer-server.exe on Windows)
├── dist/                       # Vue Web UI (served same-origin)
├── skills/                     # bundled default Skills
├── pointer-server.toml.example
├── start.sh / stop.sh / restart.sh / status.sh
└── start.ps1 / stop.ps1 / restart.ps1 / status.ps1
```

Repack the zip only (a release binary already exists):

```bash
npm run server:package
```

Packaging the License issuance tool:

```bash
npm run license-gen:build
# → target/release/license-gen-bundle/license-gen-{platform}-{arch}.zip
```

---

## 3. License key system (first time, Pointer issuer)

### 3.1 Generating the Ed25519 key pair

**The production private key is kept strictly offline and must never enter Git.**

```bash
npm run license-gen:build
# or directly on a development machine:
cargo run -p pointer-license-gen -- gen-keypair \
  --private-key /secure/offline/license.key \
  --public-key crates/pointer-core/license.pub
```

| File | Purpose |
|------|------|
| `license.key` | Offline signing private key (license-gen machine only) |
| `crates/pointer-core/license.pub` | Signature-verification public key, **compiled into pointer-server** |

After updating the public key you must **recompile and release** pointer-server; if the key is rotated, customers' old Licenses stop working.

Development environments may use the test vector documented in the repository: `tools/license-gen/dev-license.key.example` (**never for production**).

---

## 4. Issuing a License for a customer

### 4.1 Collecting the customer's machine binding information

Run this on the **target deployment machine**:

```bash
./pointer-server --machine-id          # primary binding token (fp1:…)
./pointer-server --machine-id-json     # full JSON (recommended for remote issuance)
```

**v2 fingerprint (P1/P2)**

- `machineId` = `fp1:` + SHA256(salt + os_id + board_uuid + cloud_instance)
- `machineBoardFp` / `machineCloudFp` = drift anchors: after an OS reinstall the strict fingerprint changes, but it still passes while board or cloud match
- A cloud VM reads its instance id through AWS/Azure IMDS (`169.254.169.254`)
- **Old Licenses** (a bare os id string) still work through exact match

| Platform | os_id | board | cloud |
|------|-------|-------|-------|
| Linux | `/etc/machine-id` | DMI product_uuid | AWS/Azure IMDS |
| macOS | IOPlatformUUID | IOPlatformUUID | — |
| Windows | MachineGuid | WMI BIOS UUID | — |

### 4.2 Signing commands

**Binding a specific machine (recommended in production; includes drift anchors):**

```bash
# the customer sends identity.json (from --machine-id-json)
./license-gen sign \
  --private-key /secure/offline/license.key \
  --customer-id acme-corp \
  --expires 2027-12-31 \
  --features chat,webhook,channels \
  --machine-id-json ./identity.json
```

**Binding on the signing machine itself (`--bind-machine`):**

```bash
./license-gen sign \
  --private-key /secure/offline/license.key \
  --customer-id acme-corp \
  --expires 2027-12-31 \
  --features chat,webhook,channels \
  --bind-machine
```

**Strict token only (`--machine-id fp1:…`):**

```bash
./license-gen sign \
  --private-key /secure/offline/license.key \
  --customer-id acme-corp \
  --expires 2027-12-31 \
  --features chat,webhook,channels \
  --machine-id "fp1:…"
```

**Legacy bare os id (old-version compatibility, no drift):**

```bash
./license-gen sign \
  --private-key /secure/offline/license.key \
  --customer-id acme-corp \
  --expires 2027-12-31 \
  --features chat,webhook,channels \
  --machine-id "5A372B48-8721-5807-9645-8E7A560F2518"
```

**No machine binding (testing only, use with care):**

```bash
./license-gen sign \
  --private-key /secure/offline/license.key \
  --customer-id acme-corp \
  --expires 2027-12-31 \
  --features chat,webhook,channels
```

stdout prints one line with the License Key, in the format: `base64url(payload).base64url(signature)`

License payload fields:

| Field | Description |
|------|------|
| `customer_id` | Customer identifier, e.g. `acme-corp` |
| `expires_at` | Unix timestamp (`--expires` accepts `YYYY-MM-DD` or RFC3339) |
| `features` | Feature list, commonly: `chat`, `webhook`, `channels` |
| `max_seats` | Optional, maximum number of users |
| `machine_id` | Optional, primary binding token (`fp1:…` or a legacy os id) |
| `machine_board_fp` | Optional, board drift anchor (v2) |
| `machine_cloud_fp` | Optional, cloud instance drift anchor (v2) |

---

## 5. Customer-side deployment

### 5.1 Option A: the zip package (macOS / Windows / Linux)

```bash
unzip pointer-server-linux-amd64.zip
cd pointer-server
cp pointer-server.toml.example pointer-server.toml
# edit pointer-server.toml (full template in section 6)
./start.sh          # Linux / macOS
# .\start.ps1       # Windows
```

### 5.2 Option B: the deb package (recommended on Linux)

```bash
sudo dpkg -i pointer-server_0.1.0_amd64.deb
/usr/bin/pointer-server --machine-id
sudo cp /etc/pointer-server/pointer-server.toml.example /etc/pointer-server/pointer-server.toml
sudo vi /etc/pointer-server/pointer-server.toml
sudo systemctl enable --now pointer-server
```

deb install paths:

| Path | Contents |
|------|------|
| `/usr/bin/pointer-server` | binary |
| `/usr/share/pointer-server/dist/` | Web UI |
| `/usr/share/pointer-server/skills/` | bundled Skills |
| `/etc/pointer-server/pointer-server.toml` | runtime configuration |
| `/var/lib/pointer-server/` | data directory (deb systemd default) |
| `/var/log/pointer-server/` | log directory created by postinst |

Environment variables built into the systemd unit:

```ini
Environment=POINTER_DEPLOYMENT_MODE=standalone
Environment=POINTER_APP_DATA_DIR=/var/lib/pointer-server
Environment=POINTER_SERVER_CONFIG=/etc/pointer-server/pointer-server.toml
Environment=POINTER_SERVER_STATIC_DIR=/usr/share/pointer-server/dist
Environment=POINTER_SERVER_SKILLS_DIR=/usr/share/pointer-server/skills
```

At startup `POINTER_SERVER_SKILLS_DIR` (or the `skills/` beside the zip) is synced to `{POINTER_APP_DATA_DIR}/skills/`, and then the Skill directory is loaded. If the log shows `bundled skills: no source directory found`, the bundled Skills source directory was not found.
---

## 6. Full configuration template (`pointer-server.toml`)

The template below contains **all the default sections** standalone needs. Models and API Keys do not belong in this file; fill them in after signing in under **Settings → Models**.

Configuration file lookup order:

1. The absolute path pointed to by the `POINTER_SERVER_CONFIG` environment variable
2. `pointer-server.toml` in the same directory as the executable

```toml
# =============================================================================
# pointer-server standalone production configuration template
# Copy to pointer-server.toml, then edit auth.local, license.key, public_url
# Models and API Keys are filled in after signing in, under Settings → Models
# =============================================================================

[deployment]
mode = "standalone"

[auth.local]
username = "admin"
hmac_secret = "REPLACE-WITH-LONG-RANDOM-SECRET"
# Generate: pointer-server --hash-password --secret '<hmac_secret>' '<password>'
password_hmac = "REPLACE-WITH-HMAC-HEX"

[license]
# License Key issued by Pointer (single line, payload.signature)
key = "REPLACE-WITH-LICENSE-KEY-FROM-POINTER"
# or read it from a file:
# license_file = "/etc/pointer-server/license.key"

[usage]
report_enabled = false

[server]
addr = "0.0.0.0:8787"
static_dir = "/usr/share/pointer-server/dist"
skills_dir = "/usr/share/pointer-server/skills"
# The address the browser actually uses (the HTTPS domain behind the reverse proxy, no trailing slash)
public_url = "https://pointer.acme-corp.com"
# Root directory for data persistence (deb default /var/lib/pointer-server)
app_data_dir = "/var/lib/pointer-server"
# Web branding copy (optional; see docs/ui/web-branding-welcome-elapsed.md for details)
# page_title = "Acme · AI Assistant"
# composer_placeholder = "How can I help?"
# welcome_tip_title = "I'm your expense reimbursement assistant"
# welcome_tip_body = "Once you send the attachments I'll fill in the reimbursement form for you automatically; it takes about 10–30 minutes, you can leave in the meantime, and come back to confirm the details when it's done."
# turn_elapsed_active = "Filling in the reimbursement form"
# turn_elapsed_done = "Reimbursement form filled in"
# brand_name = "Finance Assistant"
# brand_icon = "/branding/logo.png"   # shared by top-left and bottom-left
# desktop_snapshot_enabled = false    # unset = auto-detect displays
# SSE first-frame padding (enable as needed behind a buffering reverse proxy)
# sse_padding_enabled = false
# sse_padding_bytes = 10240

# Under standalone mode the [pointer] section takes no part in OAuth and can be omitted.
# If kept, it does not affect the main standalone flow:
# [pointer]
# api_base = "https://pointer-api.readflowai.com"
# oauth_client_secret = "unused-in-standalone"
```

Environment variables can override the TOML (they take precedence), see section 10.

---

## 7. Model configuration

After signing in, open **Settings → Models** and add a custom provider (address, model list, API Key, context, thinking effort, `extra_body`, …), the same as a custom provider in the desktop client. Do not configure models in the TOML or in environment variables; the old `[llm]` / `POINTER_LLM_ACTIVE_PROVIDER` are deprecated.

---

## 8. Sign-in and acceptance

### 8.1 Startup checks

```bash
# License status
curl -s http://127.0.0.1:8787/api/license/status | jq .
# expected: status = "valid"

# Service health (open in a browser when a UI is present)
curl -s -o /dev/null -w "%{http_code}" http://127.0.0.1:8787/
# expected: 200
```

### 8.2 Account and password sign-in

**Browser:** open `public_url` → welcome page or Settings → fill in account, password and captcha to sign in (matching `[auth.local]`).

**Generating password_hmac:**

```bash
pointer-server --hash-password --secret 'REPLACE-WITH-LONG-RANDOM-SECRET' 'your-password'
```

**API:**

```bash
# captcha
curl -s http://127.0.0.1:8787/api/auth/local/captcha
# login
curl -X POST http://127.0.0.1:8787/api/auth/local/login \
  -H "Content-Type: application/json" \
  -d '{"username":"admin","password":"your-password","captchaId":"...","captcha":"ABCD"}' \
  -c /tmp/pointer-cookies.txt
```

### 8.3 Feature acceptance checklist

- [ ] License `valid`, `customerId` correct
- [ ] Account + password + captcha sign-in succeeds
- [ ] Send a message and the LLM replies normally (first add a provider and fill in the API Key under Settings → Models)
- [ ] Conversation history is written to `{app_data_dir}/conversations.db`
- [ ] (Optional) Webhook automation trigger
- [ ] (Optional) after scanning to register an IM channel the WSS connects immediately (see section 9)

---

## 9. IM channels (WeCom / Feishu / DingTalk)

A standalone server supports Webhook and WSS long connections. The scan-to-register flow:

1. Web UI → Settings → IM channels → pick a channel and scan
2. On success the server **automatically** writes `channels_config.json` and restarts the WSS monitor
3. No need to restart the whole pointer-server process

Configuration file location: `{app_data_dir}/channels_config.json`

Default connection mode per channel (set to `websocket` automatically after scan-to-register):

| Channel | Vendor | Registration |
|------|--------|----------|
| WeCom | Tencent WeCom | QR scan |
| Feishu | ByteDance Feishu | QR scan |
| DingTalk | Alibaba DingTalk | QR scan |
| Weixin | Tencent iLink | QR scan (a desktop capability; the server supports it too) |

---

## 10. Full environment variable table

| Variable | Description | Typical standalone value |
|------|------|-------------------|
| `POINTER_DEPLOYMENT_MODE` | `platform` / `standalone` | `standalone` |
| `POINTER_SERVER_CONFIG` | Absolute path of the configuration file | `/etc/pointer-server/pointer-server.toml` |
| `POINTER_SERVER_ADMIN_USERNAME` | Administrator account | same as the TOML `[auth.local].username` |
| `POINTER_SERVER_ADMIN_PASSWORD_HMAC` | Password HMAC hex | same as the TOML `password_hmac` |
| `POINTER_SERVER_AUTH_HMAC_SECRET` | HMAC secret | same as the TOML `hmac_secret` |
| `POINTER_LICENSE_KEY` | License string | same as the TOML `[license].key` |
| `POINTER_LICENSE_PUBLIC_KEY` | debug only may override the embedded public key; release ignores it | do not rely on it in official packages |
| `POINTER_SERVER_ADDR` | Listen address | `0.0.0.0:8787` |
| `POINTER_SERVER_STATIC_DIR` | Static asset directory | `dist` or `/usr/share/pointer-server/dist` (TOML `[server].static_dir`) |
| `POINTER_SERVER_SKILLS_DIR` | Bundled Skills source directory | `skills` or `/usr/share/pointer-server/skills` (TOML `[server].skills_dir`) |
| `POINTER_SERVER_PUBLIC_URL` | Root URL for browser access | `https://pointer.acme-corp.com` |
| `POINTER_APP_DATA_DIR` | Data directory | `/var/lib/pointer-server` |
| `POINTER_SERVER_PAGE_TITLE` | Browser tab title | `Pointer · AI 工作台` (Pointer · AI workspace) |
| `POINTER_SERVER_COMPOSER_PLACEHOLDER` | Composer placeholder | `告诉我你想做什么` (Tell me what you want to do) |
| `POINTER_SERVER_WELCOME_TIP_TITLE` | Title of the brand-new-empty-chat welcome tip (optional) | (no tip shown) |
| `POINTER_SERVER_WELCOME_TIP_BODY` | Body of the brand-new-empty-chat welcome tip (optional) | (no tip shown) |
| `POINTER_SERVER_TURN_ELAPSED_ACTIVE` | Prefix of the elapsed chip for an active turn (optional) | `Work` |
| `POINTER_SERVER_TURN_ELAPSED_DONE` | Prefix of the elapsed chip for a finished turn (optional) | `Work` |
| `POINTER_SERVER_BRAND_NAME` | Product name in the sidebar / top bar (optional) | `Pointer` |
| `POINTER_SERVER_BRAND_ICON` | Shared logo for top-left and bottom-left (optional) | `/app-icon.png` |
| `POINTER_SERVER_DESKTOP_SNAPSHOT_ENABLED` | Desktop snapshot button (optional; unset = auto-detect displays) | automatic |
| `POINTER_SERVER_SSE_PADDING_ENABLED` | SSE first-frame padding | `false` |
| `POINTER_SERVER_SSE_PADDING_BYTES` | Number of padding bytes | `10240` |
| `POINTER_USAGE_REPORT_ENABLED` | Usage reporting | `false` |
| `POINTER_SERVER_FORBID_SESSION_USER_ID_IN_TERMINAL` | Agent `terminal` forbids `SESSION_USER_ID` in command/stdin (TOML `[server].forbid_session_user_id_in_terminal`) | off by default; set `true` when needed |
| `POINTER_SERVER_CORS_ORIGINS` | Browser CORS Origin list (TOML `[server].cors_origins`; `*` mirrors any origin) | off by default (same-origin only); turn it on for a split frontend/backend or when `web:dev` calls the API directly |

The TOML `[env]` section can inject the variables above in bulk (see `server/pointer-server.toml.example`).

---

## 11. Production: Nginx reverse-proxy example

Assume the domain `pointer.acme-corp.com` with the backend listening on `127.0.0.1:8787`:

```nginx
server {
    listen 443 ssl http2;
    server_name pointer.acme-corp.com;

    ssl_certificate     /etc/letsencrypt/live/pointer.acme-corp.com/fullchain.pem;
    ssl_certificate_key /etc/letsencrypt/live/pointer.acme-corp.com/privkey.pem;

    client_max_body_size 64m;

    location / {
        proxy_pass http://127.0.0.1:8787;
        proxy_http_version 1.1;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;
        # SSE / streaming chat (the chat event stream path is /api/chat/*/stream, not /sse)
        proxy_buffering off;
        proxy_cache off;
        proxy_read_timeout 3600s;
        proxy_send_timeout 3600s;
    }
}
```

`pointer-server`'s SSE responses carry `X-Accel-Buffering: no`, so Nginx can also disable buffering for that response in a location without `proxy_buffering off`; **`proxy_read_timeout` must still be configured on the reverse-proxy side** (the application cannot rewrite a gateway timeout). A direct connection to `pointer-server` (no reverse proxy) needs none of this.

`public_url` in `pointer-server.toml` must match the browser address:

```toml
public_url = "https://pointer.acme-corp.com"
```

---

## 12. Data directory and backups

| Item | Path |
|------|------|
| Conversation history | `{app_data_dir}/conversations.db` |
| Token usage | `{app_data_dir}/token_usage.db` |
| Task boards | `{app_data_dir}/task_boards.db` |
| Work items | `{app_data_dir}/work_items.db` |
| IM channel configuration | `{app_data_dir}/channels_config.json` |
| Session logs | `{app_data_dir}/logs/` |
| Attachment media | `{app_data_dir}/conversation-media/` |
| Memory | `{app_data_dir}/memories/` |

**Backup strategy:** snapshot the whole `{app_data_dir}` regularly; always back it up before an upgrade.

---

## 13. Hot-reloading the License (no process restart)

```bash
export POINTER_LICENSE_KEY="new_payload.new_signature"
curl -X POST http://127.0.0.1:8787/api/license/reload
curl -s http://127.0.0.1:8787/api/license/status | jq .
```

---

## 14. End-to-end flow diagram

```text
Pointer build machine                 Pointer issuer                    Customer production machine
─────────────────────                 ──────────────                    ───────────────────────────
npm run server:build
  → zip / deb delivery ──────────────────────────────────────────────→ unpack / dpkg -i
                                                                       ./pointer-server --machine-id
                                              ←──── machine ID ────────
npm run license-gen:build
./license-gen sign … ──→ License Key ─────────────────────────────────→ write into pointer-server.toml
                                                                       configure LLM api_key + auth.local
                                                                       systemctl start / ./start.sh
                                                                       account+password sign-in → acceptance
```

---

## 15. Common failures

| Symptom | Cause | Fix |
|------|------|------|
| `standalone mode requires a license key` | no License configured | fill in `[license].key` or `POINTER_LICENSE_KEY` |
| `license bound to machine_id=…` | License bound to the wrong machine | reissue with the current `--machine-id` |
| `license expired` | expired | renew and reload |
| `please sign in` / `local_login_required` | not signed in | Web UI account + password + captcha, or POST `/api/auth/local/login` |
| `invalid_credentials` | account/password or hmac mismatch | regenerate `password_hmac` with `--hash-password` |
| `invalid_captcha` | captcha wrong or expired | refresh the captcha and retry |
| Chat reports an LLM error | `api_key` invalid or not configured | check the Key in the matching provider console |
| IM scan succeeds but WSS does not connect | an old version does not persist the configuration | upgrade to a version with registration persist; or save the channel configuration manually and use `?restartMonitors=true` |
| `dpkg-deb failed` on macOS | deb is Linux-only | use the zip produced by `npm run server:build` |

---

## 16. Related documents

| Document | Reader |
|------|------|
| [`../user/standalone-server.md`](../user/standalone-server.md) | customer operations |
| [`../developer/standalone-deployment.md`](../developer/standalone-deployment.md) | development / implementation |
| [`../deploy/editions.md`](../deploy/editions.md) | four-cell packaging and deployment (including the managed server) |
| [`../contributing/cross-platform-build.md`](../contributing/cross-platform-build.md) | cross-platform build commands |

[Back to the internals index](../internals/README.md)
