# Cloud host integration (pointer-app desktop)

English | [简体中文](../../zh-CN/developer/cloud-host-integration.md)

> The rest of this document covers self-hosting and environment conventions.

The desktop client manages cloud hosts through the Pointer platform API and opens the remote pointer-server web UI in a **separate WebView window** for conversations.

## Desktop capabilities

- **Settings → Cloud host**: balance, instance list, purchase, renewal, release
- **Open**: issues an OAuth code and loads `console_url?code=&state=` in a new window
- **Switch to cloud window / Close cloud window**: return to the local main window and continue local conversations

You must first complete OAuth sign-in through the **account menu → Sign in**.

## Cloud instance environment conventions (self-hosted)

Deploy pointer-server + the Vue static assets on a Windows server, and configure:

| Item | Description |
|----|------|
| Listen | `POINTER_SERVER_ADDR=0.0.0.0:${AGENT_PORT}` |
| Health check | `GET /api/health` returns 2xx; for readiness probing you can additionally request `GET /api/ready` (requires the static UI to be mounted) |
| `AGENT_HEALTH_PATH` | Platform liveness path; set it to `/api/health` in production |
| External access | The platform assigns an external address to each instance; the desktop client opens that instance's web UI with `console_url` |
| `POINTER_API_BASE` | Platform API root address |
| `POINTER_OAUTH_CLIENT_SECRET` | Code-exchange secret matching the platform |
| `allowed_user_ids` | (recommended, required in production) only the listed platform user ids may sign in to / use the server; the corresponding env is `POINTER_SERVER_ALLOWED_USER_IDS` (comma-separated) |
| `require_allowed_users` | when `true`, startup is refused if `allowed_user_ids` is empty |

Getting the user id: leave the allow-list empty temporarily, sign in locally once and then request `GET /api/platform/session` or check the server logs; you can also look it up in the platform admin console.

Data, logs and conversation storage default to the same location as the desktop client (`{OS user data dir}/PointerApp`; `PointerAppDev` in debug builds; logs under `…/PointerApp/logs/`). There is no need to configure `app_data_dir` separately in `pointer-server.toml` unless you want to point it at a custom path (`POINTER_APP_DATA_DIR`).

The default working directory of pointer-server is `{APP_DIR}/{LOGIN_USER}`: a subdirectory of the app data directory named after the platform login user id (created automatically on the first conversation), used to isolate files when several users share one server process. Users can point a single conversation at another path from the composer.

For a Windows deployment you can put `pointer-server.toml` next to the exe (see `server/pointer-server.toml.example`); NSSM only needs to register the exe, and you do not have to write `AppEnvironmentExtra`.

When a user clicks "Open" on the desktop, the browser / WebView visits a URL carrying `code`; pointer-server performs the server-side code exchange in the SPA fallback (`POST /auth/oauth/exchange-code`), injects the LLM credentials and then 302s to `/`.

## Auth chain

```text
Desktop client Bearer → platform API (purchase / oauth-code)
WebView → cloud instance ?code= → cloud pointer-server code exchange → partner JWT + LLM keys
Conversation → cloud pointer-server reports token usage with the partner session
```

## Integration checklist

1. The desktop signs in to the platform successfully
2. The cloud instance's `/api/health` is reachable
3. After purchase the Worker liveness probe reports `app_ready`
4. "Open" exchanges the code successfully and the cloud window can send messages
5. The local main window works normally after the cloud window is closed
6. Renewal / release API behaviour matches expectations

## Web capabilities (same-origin pointer-server)

- **Attachments**: `GET /api/chat/media-download` / `media-stream` for download and inline preview
- **View desktop**: the sidebar's "View desktop" button → `POST /api/computer/manual-snapshot` (captures the screen of the host running pointer-server)

## Phase 2 (not implemented)

- On expiry **StopInstance** stops the instance and keeps the data; starting it again **StartInstance**
- A **Tab** in the main UI to switch between local / cloud host (replacing multiple windows)
