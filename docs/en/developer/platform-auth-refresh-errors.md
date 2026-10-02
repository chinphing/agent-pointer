# Platform sign-in refresh: network flakiness vs. needing to sign in again

English | [简体中文](../../zh-CN/developer/platform-auth-refresh-errors.md)

## Problem

After the access token expires, sending a message first calls `refresh_if_needed()`. If the official token-exchange endpoint fails because of network flakiness at that moment:

1. The session is still in memory / `auth.dat` still has the refresh token (**the user is not really signed out**)
2. `session_view().logged_in` becomes `false` because the access token expired
3. The old logic reported all such cases uniformly as **"Please sign in to your Pointer account first"**

From the user's side it looks like they were signed out, but in most cases it is a transient network problem.

## Current distinction

| Case | Verdict | User-facing message |
|------|------|----------|
| `http_status=401/403` / `invalid_refresh_token` | Really invalid; clear the local refresh token | Sign-in has expired / please sign in first |
| `http_status=408/429/5xx`, or a transport error with no status code | Transient failure; **keep** `auth.dat`, retry with backoff | Only report a network error if it still fails |
| Other `http_status=4xx` | No retry; do not clear the session as "signed out" | Pass through the business error text |

A failed token exchange carries a stable marker: `token exchange failed http_status=502 (...)`. The frontend's `extractPlatformAuthHttpStatus` reads that field first, to avoid misjudging on an incidental `401` in the body text.

## Usage reporting and access expiry

Desktop and web share the same `flush_unsent_reports`. A long conversation can outlive the access token (default 60 minutes; the client treats it as expired 5 minutes early), so at the end `logged_in` will be false.

Flush calls `refresh_if_needed` **only when there are pending reports and the user is not signed in**; short conversations do not exchange tokens. After a successful exchange it reports immediately; on failure the pending reports wait for next time. At the start of the next conversation turn (after the refresh) it flushes the backlog again, so it does not have to "wait for that turn to finish before uploading".

See [`../llm/token-usage-reporting.md`](../../zh-CN/llm/token-usage-reporting.md) for details.

## Unified sign-in gate (frontend / desktop)

Actions that need "signed in" to continue should not each assemble their own `ensureFreshSession` + `logged_in` check.

| Layer | Entry point | Notes |
|----|------|------|
| Frontend action | `platformAuth.requireSession({ purpose, onTransient })` | Refresh first, then validate; copy comes from `loginHint` / `formatLoginGateError` |
| Desktop IPC | `platform_auth_gate::require_logged_in` / `require_platform_user_id` | `refresh_if_needed` first, then check `logged_in` (and the user id) |
| Web HTTP | `require_platform_access` | The cookie side does not exchange tokens proactively; the browser triggers it via `ensureFreshSession` → `/api/auth/refresh` |

`onTransient`:

- `error` (default, sending a message): on a transient token-exchange failure, throw the network message instead of pretending the user is signed out
- `allow` (Composer attachments): allow through when the failure is transient and the UI still shows signed in; the backend will block once more if needed

## Attachment upload and sign-in state

Composer / pre-send goes through `requireSession`; the desktop's `save_chat_attachment` /
`save_chat_attachment_from_path` go through `require_platform_user_id`.  
Otherwise, when the access token has expired, the UI may still show signed in while the upload reports **"Please sign in to your Pointer account first"** (the UI snapshot and the Rust side's `expires_at` are out of sync).

When adding multiple files in the Composer, `requireSession({ purpose: 'attachment', maxAgeMs: 60_000 })`
reuses a session that was refreshed successfully within about 60 seconds (and still has
access-token headroom), avoiding `refreshPlatformSession` + `settings.load()` for every
file. Sending a message still forces a refresh by default
(no `maxAgeMs` passed). Concurrent refresh requests are merged into a single in-flight call.

On the web, multipart 401 / `platform_login_required` is mapped to the same sign-in hint via `formatLoginGateError(..., 'attachment')`.

## Retrying transient failures

The following two places share one backoff policy (1 immediate attempt + 3 retries: 800ms → 2s → 4s):

| Call site | Endpoint | Message when it still fails |
|--------|----------------|----------------|
| `refresh_if_needed` | `POST /auth/app/token` | Network error, cannot verify sign-in state right now, please try again later |
| `fetch_partner_balance` (`ensure_llm_allowed`) | `GET /auth/partner/balance` (before every conversation turn) | Network error, please check whether your network connection is working, then retry. |

Token exchange holds `refresh_lock`; an auth failure (401/403 / `platform_token_expired`) is **not retried** and is immediately treated as an invalid sign-in. Exhausted balance (`token_quota_exhausted`) is not retried.

Implementation points:

- `PlatformAuth::is_refresh_auth_failure`: decides by `http_status=`, not by an incidental `401` in the body
- `session_inner`: transient refresh / balance failures do not go down the "please sign in" path
- `ensure_access_token`: transient failures return the network message
- Frontend `platformAuth` / `sendUserMessage`: transient errors do not mark the UI session as signed out
