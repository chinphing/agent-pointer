# Standalone deployment: standalone-server

English | [简体中文](../../user/standalone-server.md)

pointer-server can be deployed independently of the official platform. A local build (unbound to a control plane) can be installed from source or from a local package, and you **do not** need to request a License from the issuer. Official standalone packages still need a License — see the end of this document.

Read [which-build.md](../user/which-build.md) first to confirm which one you have.

---

## Quick start

### 1. Install

Using the Linux `.deb` as an example (the package name follows the actual delivery):

```bash
sudo dpkg -i pointer-server_0.1.0_amd64.deb
```

Common paths after installation:

| Item | Path |
|------|------|
| Executable | `/usr/bin/pointer-server` |
| Configuration directory | `/etc/pointer-server/` |
| Example configuration | `/etc/pointer-server/pointer-server.toml.example` |
| Live configuration | `/etc/pointer-server/pointer-server.toml` |
| systemd service | `pointer-server` |

### 2. Edit the configuration

```bash
sudo cp /etc/pointer-server/pointer-server.toml.example /etc/pointer-server/pointer-server.toml
sudo vi /etc/pointer-server/pointer-server.toml
```

Minimal configuration example:

```toml
[deployment]
mode = "standalone"

[auth.local]
username = "admin"
hmac_secret = "replace-with-long-random-secret"
# Generate with: pointer-server --hash-password --secret '<hmac_secret>' '<password>'
password_hmac = "..."

# Unbound (non-official) builds do not require [license]. Official packages do — see below.

[usage]
report_enabled = false

[server]
addr = "0.0.0.0:8787"
public_url = "https://pointer.example.com"
# Browser tab title (optional; default Pointer · AI 工作台)
# page_title = "Acme · AI Assistant"
# Composer placeholder (optional; default 告诉我你想做什么)
# composer_placeholder = "How can I help?"
# Empty-conversation tip (optional; only on brand-new empty chats)
# welcome_tip_title = "I'm your expense reimbursement assistant"
# welcome_tip_body = "Once you send the attachments I'll fill in the reimbursement form for you automatically; it takes about 10–30 minutes, you can leave in the meantime, and come back to confirm the details when it's done."
# Turn elapsed chip prefixes (optional; default Work → "Work N m SS s")
# turn_elapsed_active = "Filling in the reimbursement form"
# turn_elapsed_done = "Reimbursement form filled in"
# Brand name / logo (optional; top-left & bottom-left share brand_icon)
# brand_name = "Finance Assistant"
# brand_icon = "/branding/logo.png"
# Desktop snapshot button (optional; unset = auto-detect display)
# desktop_snapshot_enabled = false
# SSE first-frame padding comment frame (to traverse buffering firewalls / reverse proxies; off by default, explicitly enable it with sse_padding_enabled = true when needed)
# sse_padding_enabled = false
# sse_padding_bytes = 10240
# Agent terminal: forbid SESSION_USER_ID in command/stdin (off by default)
# forbid_session_user_id_in_terminal = true
app_data_dir = "/var/lib/pointer"
```

Generate the sign-in password digest (written into `[auth.local].password_hmac`):

```bash
/usr/bin/pointer-server --hash-password --secret 'replace-with-long-random-secret' 'your-password'
# Output: password_hmac = "...."
```

### 4. Start

```bash
sudo systemctl enable --now pointer-server
sudo systemctl status pointer-server
```

Open the configured `public_url` in a browser (on the same machine you can start with `http://localhost:8787`) → sign in with account, password and captcha → **Settings → Models** → add a provider and fill in the API Key → start chatting.

Model providers in Settings are the same as the desktop client's custom providers: you can change the address, model list, key, context, maximum output, thinking effort, vision and other capabilities.

### Web branding copy (optional)

In `[server]` you can override the browser tab title, the composer placeholder, the brand-new-empty-chat welcome tip, the turn elapsed prefixes, the brand name/icon and the desktop snapshot switch (`page_title` / `composer_placeholder` / `welcome_tip_*` / `turn_elapsed_*` / `brand_name` / `brand_icon` / `desktop_snapshot_enabled`). The matching environment variables are in the "Environment variables" table below. Behaviour is documented in [`../../ui/web-branding-welcome-elapsed.md`](../../ui/web-branding-welcome-elapsed.md) next to the developer docs.

---

## License for the official standalone package

Self-hosted deployments (unbound to a control plane) can skip this section. Only the officially signed `pointer-server` enforces verification at startup.

### Getting the machine binding on first deployment (official packages only)

```bash
/usr/bin/pointer-server --machine-id-json
```

Send the output to the License issuer.

## License mechanics

### What a License is

A License is an Ed25519-signed JSON string in the format `base64(payload).base64(signature)`, containing:

- `customer_id`: customer identifier
- `expires_at`: expiry timestamp
- `features`: licensed features (such as `chat`, `webhook`, `channels`)
- `max_seats`: maximum number of users (optional)
- `machine_id`: the bound machine token (`fp1:…` or a legacy os id, optional)
- `machine_board_fp` / `machine_cloud_fp`: drift anchors (v2, still verifiable after an OS reinstall)

### License verification flow

```
At startup → read the license key → unwrap the payload → Ed25519 signature check
  → check expiry → check the machine binding → pass → run normally
```

### How to check the License status

```bash
curl http://localhost:8787/api/license/status
```

Example response:

```json
{
  "status": "valid",
  "customerId": "acme",
  "expiresAt": 1893455999,
  "features": ["chat", "webhook"],
  "machineBound": true,
  "currentMachineId": "fp1:a1b2c3..."
}
```

Possible values of `status`:

| Value | Meaning |
|----|------|
| `valid` | normal |
| `expired` | expired |
| `notConfigured` | no License configured |
| `invalid` | invalid signature |
| `machineMismatch` | machine does not match |

### Hot-reloading the License (no restart)

```bash
# set the new key
export POINTER_LICENSE_KEY="new_payload.new_signature"
# hot reload
curl -X POST http://localhost:8787/api/license/reload
```

---

## Full flow for deploying on a new machine

```
Customer side                       Issuer (administrator)
─────────────                       ──────────────────────
1. Install the delivered pointer-server package
2. Get the binding information:
   /usr/bin/pointer-server --machine-id-json > identity.json
    ──── send identity.json to the issuer ─→
                                    3. Issue a machine-bound License
    ←──── receive the License Key ──
4. Write it into /etc/pointer-server/pointer-server.toml:
   [license]
   key = "base64_payload.base64_signature"
5. systemctl enable --now pointer-server → success
```

---

## Administrator sign-in

Standalone does **not** go through the official-site cloud PC `code/state` exchange. It supports:

1. **Third-party SSO**: the portal issues a short-lived ticket and then opens `https://{public_url}/?sso=<ticket>` (configuration is in the developer doc [standalone-local-login.md](../../developer/standalone-local-login.md)).
2. **Account, password and graphical captcha** (operations fallback):

```bash
# 1) fetch a captcha
curl -s http://localhost:8787/api/auth/local/captcha
# → {"captchaId":"...","imageSvg":"<svg>...</svg>"}

# 2) sign in (read the captcha from the SVG)
curl -X POST http://localhost:8787/api/auth/local/login \
  -H "Content-Type: application/json" \
  -c /tmp/pointer-cookies.txt \
  -d '{"username":"admin","password":"your-password","captchaId":"...","captcha":"ABCD"}'
```

On success the server returns a `Set-Cookie` (`pointer_web_session`), and later requests carry the session automatically.

Configuration: `[auth.local]` (password) and the optional `[auth.local.sso]` (third-party redirect).
The old `admin_token` is deprecated and ignored.

---

## Model configuration

After signing in, open **Settings → Models → Custom providers** and add at least one provider with its API Key. As with desktop custom providers you can configure:

- Provider ID, name, API address, model list, key
- Context, maximum output, thinking effort, vision and other capabilities
- The extra parameter `extra_body` (root-level sampling parameters for local / vLLM, etc.) and per-model overrides

Saving writes to this machine's user settings, and the key is stored encrypted. Do not put models or keys in `pointer-server.toml`; the old `[llm]` section is ignored.

More parameter details are in [`../../llm/model-thinking-api.md`](../../llm/model-thinking-api.md).

---

## Data directory

`[server].app_data_dir` sets the root directory for all runtime data:

| Item | Path |
|------|------|
| Conversation history | `{app_data_dir}/conversations.db` |
| Token usage | `{app_data_dir}/token_usage.db` |
| Task boards | `{app_data_dir}/task_boards.db` |
| Work items | `{app_data_dir}/work_items.db` |
| Session logs | `{app_data_dir}/logs/` |
| Attachment media | `{app_data_dir}/conversation-media/` |
| Memory | `{app_data_dir}/memories/` |

---

## Environment variables

| Variable | Description | Default |
|------|------|--------|
| `POINTER_DEPLOYMENT_MODE` | `platform` / `standalone` | `platform` |
| `POINTER_SERVER_ADMIN_USERNAME` | Administrator account | none |
| `POINTER_SERVER_ADMIN_PASSWORD_HMAC` | Password HMAC-SHA256 hex | none |
| `POINTER_SERVER_AUTH_HMAC_SECRET` | Secret used to compute password_hmac | none |
| `POINTER_LICENSE_KEY` | License key string | none |
| `POINTER_LICENSE_PUBLIC_KEY` | **debug binaries only** may override the compiled-in public key; release packages ignore this variable | none |
| `POINTER_USAGE_REPORT_ENABLED` | Whether to report usage | `false` (standalone) |
| `POINTER_SERVER_PUBLIC_URL` | Public address of the service | inferred automatically |
| `POINTER_SERVER_PAGE_TITLE` | Browser tab title (`index.html` `<title>`) | `Pointer · AI 工作台` (Pointer · AI workspace) |
| `POINTER_SERVER_COMPOSER_PLACEHOLDER` | Default composer placeholder (written into the `pointer-composer-placeholder` meta) | `告诉我你想做什么` (Tell me what you want to do) |
| `POINTER_SERVER_WELCOME_TIP_TITLE` | Title of the brand-new-empty-chat welcome tip (optional) | (no tip shown) |
| `POINTER_SERVER_WELCOME_TIP_BODY` | Body of the brand-new-empty-chat welcome tip (optional) | (no tip shown) |
| `POINTER_SERVER_TURN_ELAPSED_ACTIVE` | Prefix of the elapsed chip for an active turn (optional) | `Work` |
| `POINTER_SERVER_TURN_ELAPSED_DONE` | Prefix of the elapsed chip for a finished turn (optional) | `Work` |
| `POINTER_SERVER_BRAND_NAME` | Product name in the sidebar / top bar (optional) | `Pointer` |
| `POINTER_SERVER_BRAND_ICON` | Shared logo for top-left and bottom-left (optional) | `/app-icon.png` |
| `POINTER_SERVER_DESKTOP_SNAPSHOT_ENABLED` | Desktop snapshot button (optional; unset = auto-detect displays) | automatic |
| `POINTER_SERVER_SSE_PADDING_ENABLED` | SSE first-frame padding (buffering reverse proxies) | `false` |
| `POINTER_SERVER_SSE_PADDING_BYTES` | Number of padding bytes | `10240` |
| `POINTER_APP_DATA_DIR` | Data directory | OS default |
| `POINTER_SERVER_STATIC_DIR` | Web UI `dist/` directory | auto-detected (including the deb's `/usr/share/pointer-server/dist`) |
| `POINTER_SERVER_SKILLS_DIR` | Bundled Skills source directory | auto-detected (including the deb's `/usr/share/pointer-server/skills`) |
| `POINTER_SERVER_ALLOWED_USER_IDS` | Platform user allowlist | empty |
| `POINTER_SERVER_CORS_ORIGINS` | Origins allowed for browser CORS (comma-separated; `*` mirrors any origin) | off (same-origin only) |

---

## FAQ

### Q: Startup reports "standalone mode requires a license key"

No License is configured. Set a valid License key in `[license].key`.

### Q: Startup reports "license bound to machine_id=... but this machine is ..."

The License is bound to another machine, so you need a License for the current machine. Run `/usr/bin/pointer-server --machine-id-json` to get the full binding information.

### Q: It says "please sign in"

The Web session is not signed in under standalone. Open the page and sign in with account, password and captcha; make sure `[auth.local]` is configured and that `password_hmac` matches `hmac_secret`.

### Q: After signing in I cannot chat / there is no model

Add a custom provider and fill in the API Key under **Settings → Models**. An `[llm]` section in the configuration file no longer takes effect.
