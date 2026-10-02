# Desktop OAuth and the official-site home page notice

English | [简体中文](../../zh-CN/developer/desktop-oauth-web-integration.md)

After the desktop client completes the loopback callback it returns a **302** to the browser, redirecting to the official-site home page with a dedicated query; the official site then shows a "signed in successfully" style notice. A normal visit to the home page does not show that notice.

## Redirect URL

```
{POINTER_WEB_BASE}/?desktop_oauth=success
```

The constants are defined in `crates/pointer-core/src/platform_auth.rs`: `DESKTOP_OAUTH_SUCCESS_QUERY`, `DESKTOP_OAUTH_SUCCESS_VALUE`.

## Official-site implementation (pointer-official)

Already implemented: `pointer-official/apps/web/src/components/DesktopOauthSuccessToast.tsx`, mounted in `app/page.tsx`.

- `SimpleToast` is shown only when `desktop_oauth=success`, then `history.replaceState` strips the query.
- A normal visit to the home page has no such parameter, so no notice is shown.

## Flow

```text
user (desktop) → bind 127.0.0.1 + local loopback self-check → open the browser authorization page
→ the official site redirects to 127.0.0.1/callback with the code
→ the desktop accept loop runs until a valid code (invalid / empty connections are ignored quickly) → 302 to the home page ?desktop_oauth=success
→ the listener stays up while the ticket is exchanged: a repeat visit likewise gets a 302 to the home page (so it never stops at 127.0.0.1)
→ the listener is closed once the exchange ends; the client leaves "waiting for authorization" as soon as the exchange succeeds
```

Each sign-in rotates the `127.0.0.1` port (scanning from 19427) so that browser keep-alive reuse cannot cause "the first sign-in succeeds, and signing in again after quitting gets stuck".

Key log prefixes: `platform_auth: bound port` / `loopback probe` / `accepted oauth callback` / `exchange start|done` / `listener closed`.
