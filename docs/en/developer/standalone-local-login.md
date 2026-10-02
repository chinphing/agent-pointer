# Standalone local login conventions

English | [简体中文](../../zh-CN/developer/standalone-local-login.md)

A standalone deployment (`deployment.mode = "standalone"`) does **not** go through the official-site cloud computer `code/state` code exchange.

Login methods:

1. **Third-party SSO** (a `?sso=` sealed ticket) — recommended for IdP / portal redirects
2. **Username + password + captcha** — ops fallback

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

Web: `POST /api/auth/local/login` → Cookie `pointer_web_session`. The user id is fixed to `local-admin`,
and carries the platform administrator flag: the sidebar can see the conversations and projects of **all users** (see [session-user-id.md](session-user-id.md) `ListScope`).

**Do not use password login for multi-user isolation.** The username/password account is a single ops identity (`local-admin`),
so everyone shares the same `SESSION_USER_ID` after logging in. To let different people see different
`SESSION_USER_ID` values in **`terminal`**, use the **third-party SSO** below, where each person's ticket carries a different stable `sub` (non-admin, seeing only their own conversations).

## Third-party SSO (standalone, unrelated to the official site)

A third party signs a short-lived JWT (HS256) with a shared secret; the browser opens:

```text
https://{agent-public-url}/?sso=<ticket>
```

Or:

```text
GET /api/auth/local/sso?sso=<ticket>
```

After verifying the signature locally the server writes a Cookie and does **not** call `POINTER_API_BASE`.

### Configuration

```toml
[auth.local.sso]
enabled = true
secret = "long-random-shared-with-idp"
# secret_prev = "previous-secret"   # optional rotation window
audience = "https://agent.example.com"   # must match aud in the ticket; public_url is recommended
# max_skew_secs = 30
```

Environment variables: `POINTER_SERVER_SSO_ENABLED`, `POINTER_SERVER_SSO_SECRET`, `POINTER_SERVER_SSO_SECRET_PREV`, `POINTER_SERVER_SSO_AUDIENCE`, `POINTER_SERVER_SSO_MAX_SKEW_SECS`.

Enabled when `secret` + `audience` are non-empty and `enabled = false` is not set explicitly.

### Ticket format

JWT compact: `base64url(header).base64url(payload).base64url(hmac-sha256)`

Header: `{"alg":"HS256","typ":"SSO"}`

Payload:

| Field | Description |
|------|------|
| `sub` | Stable user id → conversation `session_user_id` → terminal `SESSION_USER_ID` |
| `aud` | Must equal this instance's `audience` |
| `iat` / `exp` | Issued-at and expiry (a TTL of 60–120 seconds is recommended) |
| `jti` | Random one-time id (in-memory replay protection on the server) |
| `name` | Optional display name |

One stable, never-reused `sub` per person (employee id / email / IdP subject). After a verified login:

1. `user.id` in the Cookie session = `sub`
2. A new conversation writes that value through `ensure_session_user_id` (an existing value is not overwritten)
3. Injected thread-locally during `run_chat`; the `terminal` child process gets the environment variable `SESSION_USER_ID=<sub>`
4. Sidebar **projects** (including the default project and working directory) are isolated per `sub`; each person gets their own
   `{session-sandboxes}/{sub}/` default project

Switching users in the same browser: log out first, then open a new `?sso=` ticket — otherwise the original Cookie is still used.

Issuance example (ops debugging):

```bash
pointer-server --mint-sso-ticket --sub 'user-42' --name 'Alice' --ttl 120
# prints the ticket; open https://{audience}/?sso=<ticket> in a browser
```

The third party should sign on **its own server** with the same `secret`, and must not hand the key down to the browser.

### Failure

Signature verification failures and the like uniformly 302 to `/?sso_error=invalid` (or `not_configured` / `not_standalone` / `missing`); details go only to the server warn log.

## Relationship to the cloud computer

| Mode | Entry |
|------|------|
| Platform / cloud computer | `?code=&state=` → the official site's `exchange-code` |
| Standalone | `?sso=` + password login only |

See [standalone-deployment.md](standalone-deployment.md), [../user/standalone-server.md](../user/standalone-server.md), [session-user-id.md](session-user-id.md).

## Local end-to-end self-check

```bash
# SSO login for two people → verify the conversation sessionUserId; also run the in-repo test for the terminal child process env
bash scripts/e2e-standalone-sso-session-user-id.sh
cargo test -p pointer-core --lib local_sso::tests::sso_sub_flows_to_terminal_session_user_id_env -- --exact --nocapture
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
# redirect: https://agent.example.com/?sso={ticket}
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
