# Standalone local login conventions

English | [简体中文](../../zh-CN/developer/standalone-local-login.md)

A standalone deployment (`deployment.mode = "standalone"`) does **not** go through the official-site cloud computer `code/state` code exchange.

Login methods are **providers** in one registry. `pointer-server` declares them under `[auth.providers.<id>]`, and the login page renders whatever the server reports:

```text
GET  /api/auth/providers                  → { providers: [{ id, display_name, kind, The login UI is a standalone `/login` route: an unauthenticated visit to any path redirects to `/login?next=<original path>`, and a successful sign-in returns there. The page scrolls as a whole, and `token_lookup` never appears on it (the portal starts that flow).

The primary entry is set by `[auth.providers.<id>].primary = true` (solid button, first row). When unset, the priority is `platform > oidc > oauth2_userinfo > local_password`. Secondary entries are outlined; `local_password` keeps its own row and expands its form in place; two or more SSO entries collapse into "Other sign-in methods (N)". A single entry has no subtitle, several show "Choose a sign-in method", and every button reads `{name} sign in`.

icon }] }
POST /api/auth/login/start?provider=<id>  → { authorize_url }   (browser-redirect kinds)
POST /api/auth/login/finish?provider=<id> → credentials in, session out  (local_password only)
GET  /api/auth/callback/<id>              → the terminal hop for every kind
```

`login/start` only **begins** a login (a redirect URL, or a captcha challenge for `local_password`);
`login/finish` only **submits** credentials. They never share a body: credentials sent to `start` are
rejected, and `finish` on a redirect provider answers `password_login_required`
(on a `token_lookup` provider, `ticket_entry_only`).

`kind` is one of `platform`, `local_password`, `token_lookup`, `oauth2_userinfo`, `oidc`. Deployment mode injects the default: standalone → `local_password`, managed → `platform`. An explicit entry with the same id wins over the injected default, and `enabled = false` removes it.

1. **Third-party SSO** (a `token_lookup` provider) — recommended for IdP / portal redirects
2. **Username + password + captcha** (the built-in `local_password` provider) — ops fallback

Every provider also carries an `icon` for its login button. `[auth.providers.<id>].icon` wins when set; otherwise it defaults by `kind` — `platform` → `building`, `local_password` → `key`, `token_lookup` → `ticket`, `oauth2_userinfo` → `user`, `oidc` → `shield`. The SPA knows `building` / `key` / `ticket` / `user` / `shield` / `globe`; any other name falls back to a circle with the first letter of the display name.

LLM keys are entered after login in **Settings → Model configuration**, independently of the login method. The configuration file provides no model or key.

## Password login

```toml
[auth.local]
username = "admin"
hmac_secret = "long-random-secret"
password_hmac = "...."   # HMAC-SHA256 hex; never write a plaintext password
```

Generating the digest:

```bash
pointer-server --hash-password --secret '<hmac_secret>' '<password>'
```

Environment variables: `POINTER_SERVER_ADMIN_USERNAME`, `POINTER_SERVER_ADMIN_PASSWORD_HMAC`, `POINTER_SERVER_AUTH_HMAC_SECRET`.

The old fields `admin_token` / `POINTER_SERVER_ADMIN_TOKEN` are deprecated (warn at startup, then ignored).

Web: two calls, and neither one guesses from the body.

1. `POST /api/auth/login/start?provider=local_password` returns only the captcha challenge
   (`{ provider, kind, captcha: { captchaId, imageSvg } }`); it takes no credentials.
2. `POST /api/auth/login/finish?provider=local_password` with `{ username, password, captchaId, captcha }`
   verifies the captcha, then the password digest, and returns the session view plus
   `Set-Cookie: pointer_web_session`. Failures: `401 invalid_captcha` / `401 invalid_credentials`.

The user id is fixed to `local-admin`,
and carries the platform administrator flag: the sidebar can see the conversations and projects of **all users** (see [session-user-id.md](session-user-id.md) `ListScope`).

**Do not use password login for multi-user isolation.** The username/password account is a single ops identity (`local-admin`),
so everyone shares the same `SESSION_USER_ID` after logging in. To let different people see different
`SESSION_USER_ID` values in **`terminal`**, use the **third-party SSO** below, where each person's ticket carries a different stable `sub` (non-admin, seeing only their own conversations).

## Third-party SSO (standalone, unrelated to the official site)

A third party signs a short-lived JWT (HS256) with a shared secret and sends the browser to that provider's callback:

```text
https://{agent-public-url}/api/auth/callback/portal?ticket=<ticket>
```

After verifying the signature locally the server writes a Cookie and does **not** call `POINTER_API_BASE`.

### Configuration

```toml
[auth.providers.portal]
kind = "token_lookup"
display_name = "SSO"                        # optional; defaults to the provider id
# callback_path = "/api/auth/callback/portal"   # optional; this is already the default

[auth.providers.portal.token_lookup]
ticket_param = "ticket"        # query parameter carrying the ticket (default)
subject_path = "/sub"          # stable user id claim (default)
nickname_path = "/name"        # optional display name claim
# clock_skew_secs = 30         # tolerance for exp / iat (default 30)

[auth.providers.portal.token_lookup.signing]
secret = "long-random-shared-with-idp"
# secret_prev = "previous-secret"   # optional rotation window
audience = "https://agent.example.com"   # must match aud in the ticket; public_url is recommended
```

`token_lookup` accepts **either** `signing.secret` (signed JWT, the case below) **or** `lookup.url` (an opaque ticket verified by a server-to-server call) — never both, and startup fails when neither is set.

`POINTER_SERVER_SSO_SECRET` / `POINTER_SERVER_SSO_AUDIENCE` only supply defaults to `--mint-sso-ticket`; the provider itself reads the TOML above.

### Ticket format

JWT compact: `base64url(header).base64url(payload).base64url(hmac-sha256)`

Header: `{"alg":"HS256","typ":"SSO"}`

Payload:

| Field | Description |
|------|------|
| `sub` | Stable user id; the session user id becomes `<provider-id>:<sub>` → conversation `session_user_id` → terminal `SESSION_USER_ID` |
| `aud` | Must equal this instance's `audience` |
| `iat` / `exp` | Issued-at and expiry (a TTL of 60–120 seconds is recommended) |
| `jti` | Random one-time id (in-memory replay protection on the server) |
| `name` | Optional display name |

One stable, never-reused `sub` per person (employee id / email / IdP subject). After a verified login:

1. `user.id` in the Cookie session = `<provider-id>:<sub>`
2. A new conversation writes that value through `ensure_session_user_id` (an existing value is not overwritten)
3. Injected thread-locally during `run_chat`; the `terminal` child process gets the environment variable `SESSION_USER_ID=<provider-id>:<sub>`
4. Sidebar **projects** (including the default project and working directory) are isolated per user id; each person gets their own
   `{session-sandboxes}/{user-id}/` default project

Switching users in the same browser: log out first, then open a new callback URL with a fresh ticket — otherwise the original Cookie is still used.

Issuance example (ops debugging):

```bash
pointer-server --mint-sso-ticket --sub 'user-42' --name 'Alice' --ttl 120
# prints the ticket; open https://{audience}/api/auth/callback/portal?ticket=<ticket> in a browser
```

The third party should sign on **its own server** with the same `secret`, and must not hand the key down to the browser.

### Failure

Signature verification failures and the like uniformly 302 to `/?platform_login_error=<code>`, where `code` is one of `unknown_provider`, `provider_disabled`, `invalid_state`, `missing_code`, `provider_denied`, `exchange_failed`, `server_access_denied`, `password_login_required`, `ticket_entry_only`, `ticket_missing`, `ticket_expired`, `ticket_replayed`, `ticket_signature_invalid`, `ticket_invalid`, `upstream_error`; details go only to the server warn log.

On success the browser lands on `/?auth=success` (the SPA strips the query); the older `platform_login=success` marker is still accepted when reading.

## Login rate limiting

`[auth.rate_limit]` (on by default) throttles the two login endpoints. The counters live in **process memory**, so they are per instance and reset on restart.

| Key | Default | Effect |
|------|----------|------|
| `enabled` | `true` | Master switch |
| `per_ip_per_minute` | `20` | `login/start` calls per client IP per minute (`0` = no limit) |
| `max_pending` | `256` | In-flight logins (`0` = no limit); over capacity the new request is **rejected**, never by evicting one in progress |
| `finish_fail_backoff_secs` | `3` | Cooldown after a failed `login/finish` (`invalid_captcha` / `invalid_credentials`); a success clears it (`0` = off) |
| `trusted_proxies` | empty | IPs / CIDRs whose `X-Forwarded-For` is trusted |

Each rejection answers **429** with a plain-text code as the body:

| Code | Cause |
|------|------|
| `rate_limited` | more than `per_ip_per_minute` starts from that IP this minute |
| `too_many_attempts` | that IP is inside the failure cooldown |
| `login_capacity_reached` | `max_pending` logins are already in flight |

`X-Forwarded-For` is read **only** when the direct peer is listed in `trusted_proxies`, and the client is then the rightmost hop that is not itself trusted — a caller cannot pick its own bucket by appending a header value. Behind a reverse proxy, list the proxy's address (exact IP or a CIDR such as `10.0.0.0/8`), otherwise every user shares the proxy's bucket.

## Single instance and in-memory login state

Everything behind the login flow lives in **process memory**:

| Store | Contents | Lifetime |
|------|------|------|
| `web_sessions` | issued web sessions (the `pointer_web_session` cookie value) | until logout or restart |
| `auth_pending` | a login that was started but not finished (PKCE verifier, `state`, `nonce`) | 600 s (`OAUTH_PENDING_TTL`) |
| captcha challenges | the `local_password` challenge, single-use | a few minutes, in memory |

Consequences:

- **Run exactly one `pointer-server` process.** A second instance — another port, a replica behind a
  load balancer, a rolling deploy with two pods alive at once — does not see the other one's sessions or
  pending logins. The browser is bounced back to the login page, or the callback fails with `invalid_state`.
- **A restart signs everyone out.** No session survives an upgrade, a redeploy or `systemctl restart`.
- A captcha fetched from one instance cannot be finished on another.

`pointer-server auth doctor` reports this as its session-store check; see [cli.md](cli.md).

## Relationship to the cloud computer

| Mode | Entry |
|------|------|
| Platform / cloud computer | the `platform` provider: `POST /api/auth/login/start?provider=platform` → the official site's `exchange-code` |
| Standalone | the `local_password` and `token_lookup` providers only |

See [standalone-deployment.md](standalone-deployment.md), [../user/standalone-server.md](../user/standalone-server.md), [session-user-id.md](session-user-id.md).

## Local end-to-end self-check

```bash
# SSO login for two people → verify the conversation sessionUserId; also run the provider unit tests
bash scripts/e2e-standalone-sso-session-user-id.sh
cargo test -p pointer-core auth_provider::
```

## Minimal ticket issuance examples

The header is fixed to `{"alg":"HS256","typ":"SSO"}` (matching the server).

### Python

```python
import base64, hashlib, hmac, json, time, uuid

def b64url(data: bytes) -> str:
    return base64.urlsafe_b64encode(data).rstrip(b"=").decode()

def mint_sso(secret: str, aud: str, sub: str, name: str | None = None, ttl: int = 120) -> str:
    now = int(time.time())
    header = b64url(b'{"alg":"HS256","typ":"SSO"}')
    payload = {
        "sub": sub, "aud": aud, "iat": now, "exp": now + ttl,
        "jti": str(uuid.uuid4()),
    }
    if name:
        payload["name"] = name
    body = b64url(json.dumps(payload, separators=(",", ":")).encode())
    sig = b64url(hmac.new(secret.encode(), f"{header}.{body}".encode(), hashlib.sha256).digest())
    return f"{header}.{body}.{sig}"

# ticket = mint_sso("shared-secret", "https://agent.example.com", "user-42", "Alice")
# redirect: https://agent.example.com/api/auth/callback/portal?ticket={ticket}
```

### Java

```java
import javax.crypto.Mac;
import javax.crypto.spec.SecretKeySpec;
import java.nio.charset.StandardCharsets;
import java.util.Base64;
import java.util.UUID;

static String b64url(byte[] data) {
    return Base64.getUrlEncoder().withoutPadding().encodeToString(data);
}

static String mintSso(String secret, String aud, String sub, String name, int ttlSecs) throws Exception {
    long now = System.currentTimeMillis() / 1000;
    String header = b64url("{\"alg\":\"HS256\",\"typ\":\"SSO\"}".getBytes(StandardCharsets.UTF_8));
    String json = "{\"sub\":\"" + sub + "\",\"aud\":\"" + aud + "\",\"iat\":" + now
            + ",\"exp\":" + (now + ttlSecs) + ",\"jti\":\"" + UUID.randomUUID() + "\""
            + (name == null ? "" : ",\"name\":\"" + name + "\"") + "}";
    String body = b64url(json.getBytes(StandardCharsets.UTF_8));
    Mac mac = Mac.getInstance("HmacSHA256");
    mac.init(new SecretKeySpec(secret.getBytes(StandardCharsets.UTF_8), "HmacSHA256"));
    String sig = b64url(mac.doFinal((header + "." + body).getBytes(StandardCharsets.UTF_8)));
    return header + "." + body + "." + sig;
}
```
