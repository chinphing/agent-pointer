# Standalone 本地登录约定

[English](../../en/developer/standalone-local-login.md) | 简体中文

独立部署（`deployment.mode = "standalone"`）**不走**官网云电脑 `code/state` 换码。

登录方式来自服务端的 provider 注册表（`[auth.providers.<id>]`）。登录页按
`GET /api/auth/providers` 返回的列表逐个渲染，没有写死的入口：

1. **`token_lookup`**（第三方门户短时票）— 推荐给 IdP / 门户跳转
2. **`local_password`**（账号密码 + 验证码）— 运维备用

登录页是独立的 `/login` 路由：未登录访问任意路径会 302 到 `/login?next=<原路径>`，登录成功后回到该路径；页面整页可滚动，`token_lookup` 不会出现在页面上（门户票据入口由门户发起）。

主入口用 `[auth.providers.<id>].primary = true` 指定（实心按钮、排第一行）；不指定时按 `platform > oidc > oauth2_userinfo > local_password` 的优先级挑一个。次级入口描边显示，`local_password` 独占一行并在点击后就地展开表单，其余 SSO 类 ≥2 个时合并成「其他登录方式（N）」行内展开。只有一个入口时没有副标题，多个入口时副标题为「选择登录方式」，按钮文案统一为 `{name}登录`。

每个 provider 还带一个 `icon`（登录按钮图标名）：`[auth.providers.<id>].icon` 显式配置优先，未配置时按 `kind` 默认 —— `platform` → `building`、`local_password` → `key`、`token_lookup` → `ticket`、`oauth2_userinfo` → `user`、`oidc` → `shield`。前端认识 `building` / `key` / `ticket` / `user` / `shield` / `globe`，其余名字回落为「显示名首字母圆标」。

LLM 密钥在登录后于 **设置 → 模型配置** 填写，与登录方式无关。配置文件不提供模型或密钥。

## 单实例与会话内存

`web_sessions`（浏览器会话）与 `auth_pending`（登录中状态，TTL 600s）都只保存在进程内存里：

- 只支持**单实例**部署：不要多副本、不要把请求负载均衡到多个进程（同一个人落到另一个实例就是未登录）。
- 进程重启 = 所有在线会话失效、未完成的登录失效，浏览器需要重新登录。
- 本版本**不提供外置会话存储**；要横向扩展必须先补这一层。
- 部署前自检：`pointer-server auth doctor`（只读，默认不联网；加 `--network` 才检查出网连通性）。

## 登录入口限流

`[auth.rate_limit]`（默认开启）对两个登录端点限流；计数在**进程内存**里，按实例、重启清零。

| 键 | 默认 | 作用 |
|------|------|------|
| `enabled` | `true` | 总开关；`false` 时按 IP 计数、在途上限与冷却全部关闭 |
| `per_ip_per_minute` | `20` | 每 IP 每分钟允许的 `login/start` 次数（`0` = 不按 IP 限） |
| `max_pending` | `256` | 同时在途的登录上限（`0` = 不限） |
| `finish_fail_backoff_secs` | `3` | `login/finish` 失败（`invalid_captcha` / `invalid_credentials`）后的冷却秒数，成功即清除（`0` = 关闭） |
| `trusted_proxies` | 空 | 采信其 `X-Forwarded-For` 的代理地址（精确 IP 或 CIDR） |

被拒时一律 **429**，body 是纯文本错误码：

| 错误码 | 含义 |
|------|------|
| `rate_limited` | 该 IP 本分钟 `login/start` 次数超过 `per_ip_per_minute` |
| `too_many_attempts` | 该 IP 处于 `finish_fail_backoff_secs` 冷却中 |
| `login_capacity_reached` | 在途登录已达 `max_pending` |

超过 `max_pending` 是**拒绝新请求**，绝不淘汰进行中的登录 —— 淘汰会让那个用户拿到莫名的 `invalid_state`。

只有**直连对端 IP 命中 `trusted_proxies`** 时才读 `X-Forwarded-For`，并取最右侧非可信跳作为客户端 IP，客户端自己在左边塞条目无效；其余请求一律按 socket 地址计数。反向代理后面部署时记得把代理地址（精确 IP，或 `10.0.0.0/8` 这类 CIDR）列进去，否则所有用户共用代理 IP 的额度。

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

Web 登录分两步，`login/start` 只发起、`login/finish` 只提交：

```text
POST /api/auth/login/start?provider=local_password
  → { provider, kind, captcha: { captchaId, imageSvg } }   （内存一次性，TTL 5min）

POST /api/auth/login/finish?provider=local_password
  { username, password, captchaId, captcha }
  → 成功：Set-Cookie pointer_web_session
  → 失败：401 invalid_captcha | invalid_credentials
```

用户 id 固定为 `local-admin`，
且带平台管理员标志：侧栏可看到**所有用户**的会话与项目（见 [session-user-id.md](session-user-id.md) `ListScope`）。

**多人隔离不要用密码登录。** 账号密码只有一个运维身份（`local-admin`），
所有人登录后 `SESSION_USER_ID` 相同。要让不同人在 **`terminal`** 里看到不同的
`SESSION_USER_ID`，请用下方 **第三方 SSO**，每人票里带不同的稳定 `sub`（非管理员，只看自己的会话）。

## 第三方 SSO（独立，与官网无关）

第三方用共享密钥签发短时 JWT（HS256），浏览器打开统一回调入口：

```text
https://{agent-public-url}/api/auth/callback/portal?ticket=<ticket>
```

`portal` 是配置里 `[auth.providers.<id>]` 的 id；`ticket` 是 `ticket_param` 的默认值。
服务端本地验签后写 Cookie，**不调用** `POINTER_API_BASE`。

### 配置

```toml
[auth.providers.portal]
kind = "token_lookup"
display_name = "SSO"

[auth.providers.portal.token_lookup]
ticket_param = "ticket"
subject_path = "/sub"
nickname_path = "/name"

[auth.providers.portal.token_lookup.signing]
secret = "long-random-shared-with-idp"
# secret_prev = "previous-secret"   # 轮换窗口可选
audience = "https://agent.example.com"   # 须与票里 aud 一致，建议用 public_url
```

验签密钥只走 TOML（`signing.*`），没有环境变量形式；`enabled = false` 可停用该 provider。
`--mint-sso-ticket` 造票时缺省读 `POINTER_SERVER_SSO_SECRET` / `POINTER_SERVER_SSO_AUDIENCE`。

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

每人一个稳定、不复用的 `sub`（工号 / 邮箱 / IdP subject）。验签登录后：

1. Cookie 会话里的 `user.id` = `sub`
2. 新对话 `ensure_session_user_id` 写入该值（已有值不覆盖）
3. `run_chat` 时线程局部注入；`terminal` 子进程环境变量 `SESSION_USER_ID=<sub>`
4. 侧栏 **项目**（含默认项目与工作目录）按 `sub` 隔离；每人各自的
   `{session-sandboxes}/{sub}/` 默认项目

同一浏览器换人：先退出再打开新的 `ticket` 入口，否则仍用原 Cookie。

签发示例（运维调试）：

```bash
pointer-server --mint-sso-ticket --sub 'user-42' --name 'Alice' --ttl 120
# 输出 ticket；浏览器打开 https://{public_url}/api/auth/callback/portal?ticket=<ticket>
```

第三方应在**己方服务端**用同一 `secret` 签发，勿把密钥下发浏览器。

### 失败

验签失败等统一 302 `/?sso_error=invalid`（或 `not_configured` / `not_standalone` / `missing`），细节只打服务端 warn 日志。

## 与云电脑的关系

| 模式 | 入口 |
|------|------|
| Platform / 云电脑 | `?code=&state=` → 官网 `exchange-code` |
| Standalone | 仅 `token_lookup` 票 + `local_password` 密码登录 |

详见 [standalone-deployment.md](standalone-deployment.md)、[../user/standalone-server.md](../user/standalone-server.md)、[session-user-id.md](session-user-id.md)。

## 本地端到端自检

```bash
# SSO 登录两人 → 校验会话 sessionUserId；另跑库内测试验证 terminal 子进程 env
bash scripts/e2e-standalone-sso-session-user-id.sh
cargo test -p pointer-core --lib local_sso::tests::sso_sub_flows_to_terminal_session_user_id_env -- --exact --nocapture
```

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
