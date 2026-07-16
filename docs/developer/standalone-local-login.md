# Standalone 本地登录约定

独立部署（`deployment.mode = "standalone"`）**不走**官网云电脑 `code/state` 换码。

登录方式：

1. **第三方 SSO**（`?sso=` 密封票）— 推荐给 IdP / 门户跳转
2. **账号密码 + 验证码** — 运维备用

LLM 密钥始终来自本地 `[llm]` 配置，与登录无关。

## 密码登录

```toml
[auth.local]
username = "admin"
hmac_secret = "long-random-secret"
password_hmac = "...."   # HMAC-SHA256 hex；勿写明文密码
```

生成摘要：

```bash
pointer-server --hash-password --secret '<hmac_secret>' '<password>'
```

环境变量：`POINTER_SERVER_ADMIN_USERNAME`、`POINTER_SERVER_ADMIN_PASSWORD_HMAC`、`POINTER_SERVER_AUTH_HMAC_SECRET`。

旧字段 `admin_token` / `POINTER_SERVER_ADMIN_TOKEN` 已废弃（启动 warn，忽略）。

Web：`POST /api/auth/local/login` → Cookie `pointer_web_session`。用户 id 固定为 `local-admin`。

## 第三方 SSO（独立，与官网无关）

第三方用共享密钥签发短时 JWT（HS256），浏览器打开：

```text
https://{agent-public-url}/?sso=<ticket>
```

或：

```text
GET /api/auth/local/sso?sso=<ticket>
```

服务端本地验签后写 Cookie，**不调用** `POINTER_API_BASE`。

### 配置

```toml
[auth.local.sso]
enabled = true
secret = "long-random-shared-with-idp"
# secret_prev = "previous-secret"   # 轮换窗口可选
audience = "https://agent.example.com"   # 须与票里 aud 一致，建议用 public_url
# max_skew_secs = 30
```

环境变量：`POINTER_SERVER_SSO_ENABLED`、`POINTER_SERVER_SSO_SECRET`、`POINTER_SERVER_SSO_SECRET_PREV`、`POINTER_SERVER_SSO_AUDIENCE`、`POINTER_SERVER_SSO_MAX_SKEW_SECS`。

`secret` + `audience` 非空且未显式 `enabled = false` 时启用。

### Ticket 格式

JWT compact：`base64url(header).base64url(payload).base64url(hmac-sha256)`

Header：`{"alg":"HS256","typ":"SSO"}`

Payload：

| 字段 | 说明 |
|------|------|
| `sub` | 用户稳定 id → 对话 `session_user_id` → terminal `SESSION_USER_ID` |
| `aud` | 须等于本实例 `audience` |
| `iat` / `exp` | 签发与过期（建议 TTL 60–120 秒） |
| `jti` | 随机一次性 id（服务端内存防重放） |
| `name` | 可选昵称 |

签发示例（运维调试）：

```bash
pointer-server --mint-sso-ticket --sub 'user-42' --name 'Alice' --ttl 120
# 输出 ticket；浏览器打开 https://{audience}/?sso=<ticket>
```

第三方应在**己方服务端**用同一 `secret` 签发，勿把密钥下发浏览器。

### 失败

验签失败等统一 302 `/?sso_error=invalid`（或 `not_configured` / `not_standalone` / `missing`），细节只打服务端 warn 日志。

## 与云电脑的关系

| 模式 | 入口 |
|------|------|
| Platform / 云电脑 | `?code=&state=` → 官网 `exchange-code` |
| Standalone | 仅 `?sso=` + 密码登录 |

详见 [standalone-deployment.md](standalone-deployment.md)、[../user/standalone-server.md](../user/standalone-server.md)、[session-user-id.md](session-user-id.md)。

## 签发 ticket 示例（最短）

Header 固定为 `{"alg":"HS256","typ":"SSO"}`（与服务端一致）。

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
