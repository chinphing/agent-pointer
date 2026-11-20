# 独立部署开发文档

[English](../../en/developer/standalone-deployment.md) | 简体中文

pointer-server 支持**脱离官方平台独立部署**。本文档覆盖架构、配置格式、API 端点与实现模块。

> 本文讲的是 `pointer-server`。控制面是另一个二进制 `pointer-admin`，它的部署与客户端对接见 [企业内网部署](../deploy/enterprise.md)。

## 目录

- [部署模式（deployment_mode.rs）](#部署模式)
- [本地认证（local_auth.rs）](#本地认证)
- [License 系统（license/）](#license-系统)
- [模型配置](#模型配置)
- [API 端点](#api-端点)
- [配置参考](#配置参考)
- [改造文件清单](#改造文件清单)

---

## 部署模式

**文件：** `crates/pointer-core/src/deployment_mode.rs`


| 模式             | 说明                                                                                 |
| -------------- | ---------------------------------------------------------------------------------- |
| `platform`（已绑定时默认） | 连接控制面（仅 **managed** 口味预填域名），OAuth + 云端 Key |
| `standalone`   | 本地账号密码 + Web 设置里的模型密钥。**managed** 口味的 standalone 仍校验 License；未设置口味（自建 / 未绑定）不强制 |


**余额 / LLM 门禁：** 官方账户余额校验（`ensure_llm_allowed`、`GET /auth/partner/llm-credentials`）仅在 `platform` 模式生效；`standalone` 下为 no-op，不访问官方余额 API。

```rust
pub fn deployment_mode() -> Mode;
pub fn is_standalone() -> bool;
```

配置来源优先级：

1. `POINTER_DEPLOYMENT_MODE` 环境变量（`platform` / `standalone`）
2. `[deployment].mode` 配置文件值
3. 都没有时按控制面绑定推导：已绑定 → `platform`，未绑定 → `standalone`

---



## 本地认证

**文件：** `crates/pointer-core/src/local_auth.rs`、`server/src/auth_routes.rs`

Standalone 模式下替代官方 OAuth。配置存 `username` + `password_hmac`（HMAC-SHA256 hex）+ `hmac_secret`，不存明文密码。

```rust
pub fn hmac_sha256_hex(secret: &str, password: &str) -> String;
pub fn verify_local_password(username: &str, password: &str) -> bool;
pub fn local_password_auth_configured() -> bool;
```

运维生成摘要：

```bash
pointer-server --hash-password --secret '<hmac_secret>' '<password>'
```

**流程：**

```
GET  /api/auth/providers        → { providers: [{ id, display_name, kind }] }
GET  /api/auth/mode             → { "mode": "standalone" | "platform" }
POST /api/auth/login/start?provider=local_password
  → { provider, kind, captcha: { captchaId, imageSvg } }  （内存一次性，TTL 5min；不接收凭据）
POST /api/auth/login/finish?provider=local_password
  { username, password, captchaId, captcha }
  → 校验验证码 → verify_local_password
  → 成功：创建 WebSession（auth_kind = Local）+ Set-Cookie
  → 失败：401 invalid_captcha | invalid_credentials
```

**Session：** `WebSessionAuthKind::Local`；所有需登录 API 在 standalone 下要求本地会话。

**单实例约束：** 会话（`web_sessions`）与登录中状态（`auth_pending`，TTL 600s）只在进程内存，
不支持多副本/负载均衡；进程重启会让在线会话与未完成的登录失效。部署前跑
`pointer-server auth doctor`（只读，`--network` 才做出网检查）确认配置可用。

旧版 `admin_token` / `POINTER_SERVER_ADMIN_TOKEN` 已废弃（启动 warn 并忽略）。

### Standalone 模式下 LLM 凭证路径

在 `chat_service/session_inner.rs` 中：

- standalone + web_session + `has_local_llm` → 跳过 `ensure_llm_allowed()`（无平台 quota 检查）
- standalone + 无本地 LLM Key → 报错提示配置 LLM

---



## License 系统

**文件：** `crates/pointer-core/src/license/mod.rs` + `verify.rs` + `fingerprint.rs`

### Ed25519 离线签名方案

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



### 模块结构

```rust
pub struct LicenseClaims {
    pub customer_id, pub expires_at, pub features, pub max_seats,
    pub machine_id, pub machine_board_fp, pub machine_cloud_fp,
}
pub struct MachineFactors { pub os_id, pub board_uuid, pub cloud_provider, pub cloud_instance_id }
pub struct MachineFingerprints { pub strict, pub board, pub cloud }  // fp1:… + drift anchors

// 核心函数
pub fn validate_license_at_startup() -> Result<()>
pub fn reload_license_from_env() -> Result<LicenseStatusView>
pub fn current_machine_id() -> anyhow::Result<String>           // fp1:… binding token
pub fn current_machine_identity() -> Result<MachineIdentityView> // --machine-id-json
pub fn verify_machine_binding(...) -> Result<()>
```



### 验证流程

```
main()
  → deployment_mode == standalone?（且仅 managed 口味强制，见上）
    → 读取 POINTER_LICENSE_KEY（来自 [license].key，或 [license].license_file 指向的文件）
    → base64url 解码 payload 和 signature
    → Ed25519 验签（公钥编译嵌入 license.pub；仅 debug 构建允许 POINTER_LICENSE_PUBLIC_KEY 覆盖，release 忽略）
    → 检查 expiry
    → 如果 claims.machine_id 不为空，校验 v2 指纹或 legacy os id
    → v2：strict match 或 board/cloud 漂移锚点通过
    → 缓存 claims → 运行
```

**公钥约定：** 正式 release 二进制只认编译嵌入的 `license.pub`。
`POINTER_LICENSE_PUBLIC_KEY` 仅在 **debug**（`debug_assertions`）下可覆盖，供本地/e2e 自签；
release 若设置该变量会打 warn 并忽略。


### 机器绑定（v2 指纹）

`fingerprint.rs` 收集多信号并生成 salted SHA-256：


| 信号         | Linux             | macOS          | Windows       | 云 VM           |
| ---------- | ----------------- | -------------- | ------------- | -------------- |
| os_id      | `/etc/machine-id` | IOPlatformUUID | MachineGuid   | 同左             |
| board_uuid | DMI product_uuid  | IOPlatformUUID | WMI BIOS UUID | —              |
| cloud      | —                 | —              | —             | AWS/Azure IMDS |


- **strict**（`machine_id`）：`fp1:` + SHA256(salt + os + board + cloud)
- **漂移锚点**：`machine_board_fp`、`machine_cloud_fp` — OS 重装后 strict 变化仍可验证
- **Legacy**：裸 os id 字符串 exact match 仍兼容

```bash
./pointer-server --machine-id-json   # 推荐远程签发
./pointer-server --machine-id        # 仅 fp1:… token
```



### License 生成 CLI

**文件：** `tools/license-gen/`

```bash
# 生成密钥对（私钥严格离线保存，不进仓库）
cargo run -p pointer-license-gen -- gen-keypair \
  --private-key license.key \
  --public-key crates/pointer-core/license.pub

# 签发（不绑定机器）
cargo run -p pointer-license-gen -- sign \
  --private-key license.key \
  --customer-id acme \
  --expires 2027-12-31 \
  --features chat,webhook,channels

# 签发（绑定指定机器 JSON，推荐）
cargo run -p pointer-license-gen -- sign \
  --private-key license.key \
  --customer-id acme \
  --expires 2027-12-31 \
  --features chat,webhook \
  --machine-id-json ./identity.json

# 签发（绑定当前机器，含漂移锚点）
cargo run -p pointer-license-gen -- sign \
  --private-key license.key \
  --customer-id acme \
  --expires 2027-12-31 \
  --features chat,webhook \
  --bind-machine

# 签发（legacy 裸 os id，无漂移）
cargo run -p pointer-license-gen -- sign \
  --private-key license.key \
  --customer-id acme \
  --expires 2027-12-31 \
  --machine-id "5A372B48-8721-5807-9645-8E7A560F2518"
```



### 热加载（不重启）

```bash
# 设置新的 license key
export POINTER_LICENSE_KEY="new_payload.new_signature"
# 或修改 pointer-server.toml [license].key
curl -X POST http://localhost:8787/api/license/reload
```

---



## 模型配置

Standalone **不从** `pointer-server.toml` 读取模型、地址或 API Key：配置解析器里没有 `[llm]` 字段，未知段被 serde 静默丢弃 —— **旧 `[llm]` 段既不生效，也不报错、不告警**；旧变量 `POINTER_LLM_ACTIVE_PROVIDER` 同样不再映射。模型服务只在**设置**里配，见 [`../user/model-providers.md`](../user/model-providers.md)。

登录后打开 **设置 → 模型配置 → 自定义服务**，与桌面客户端自定义服务相同：添加服务商（id、名称、API 地址、模型名单、密钥），以及上下文、最大输出、思考强度、能力勾选、`extra_body` 和单模型覆盖。保存走 `updateUserSettings`，写入 `user_settings.json`；密钥以 `enc:v1:` 加密落盘。

本地默认不再内置任何平台服务商/模型（`activeProviderId` / `model` 默认为空）。未在界面添加服务并填写密钥时，对话不可用。

---



## 平台副作用管理


| 模块                                   | Standalone 行为                                           |
| ------------------------------------ | ------------------------------------------------------- |
| `platform_endpoints.rs`              | 不回落官方默认域名（源码不硬编码，未注入即未绑定），未配置时 warn                       |
| `cloud_agent_auth.rs`                | 禁用云 OAuth code exchange                                 |
| `token_usage_store.rs`               | `usage_report_enabled()` 默认 false，跳过上报                  |
| `media/oss.rs`                       | 从 `[media_oss]` 或 `OSS_*` 环境变量读取，不从 OAuth 注入            |
| `experiences.rs`                     | **不加载**官方经验目录（直接返回空，欢迎页不展示经验区）                    |


---



## API 端点


| 端点                        | 方法   | 说明                                                         |
| ------------------------- | ---- | ---------------------------------------------------------- |
| `/api/auth/mode`          | GET  | `{ "mode": "standalone" | "platform" }`                    |
| `/api/auth/providers`     | GET  | `{ providers: [{ id, display_name, kind }] }`，登录页按此渲染  |
| `/api/auth/login/start`   | POST | `?provider=<id>`；跳转类返回 `authorize_url`，`local_password` 只返回验证码挑战 |
| `/api/auth/login/finish`  | POST | `?provider=<id>`；`local_password` 提交 `{ username, password, captchaId, captcha }`，返回 Set-Cookie |
| `/api/auth/callback/<id>` | GET  | 统一回调；`platform` 默认 `/api/auth/oauth/callback`（`callback_path` 可改） |
| `/api/license/status`     | GET  | 返回 license claims、状态、机器绑定信息                                |
| `/api/license/reload`     | POST | 从 `POINTER_LICENSE_KEY` 热加载新 license                       |




### License Status 响应

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



## 配置参考

完整示例见 `[server/pointer-server.toml.example](../../../server/pointer-server.toml.example)`。

```toml
[deployment]
mode = "standalone"                    # platform（默认）| standalone

[auth.local]
username = "admin"
hmac_secret = "replace-with-long-random-secret"
# pointer-server --hash-password --secret '<hmac_secret>' '<password>'
password_hmac = "...."

# 登录方式：每个 provider 一个区块，登录页按 `GET /api/auth/providers` 渲染
# [auth.providers.local_password]
# kind = "local_password"              # standalone 默认注入，可显式覆盖
# display_name = "账号密码"
#
# [auth.providers.portal]
# kind = "token_lookup"                # 第三方门户短时票
# display_name = "SSO"
# [auth.providers.portal.token_lookup]
# ticket_param = "ticket"              # 入口查询参数名
# subject_path = "/sub"                # 稳定用户 id 的 claim
# nickname_path = "/name"              # 可选显示名 claim
# [auth.providers.portal.token_lookup.signing]
# secret = "shared-with-portal"        # 票签名密钥（env 无对应项）
# secret_prev = "rotating-old-secret"  # 轮换窗口内仍接受的上一版密钥
# audience = "pointer-server"          # 须与票的 aud 一致
# 入口：GET https://{public_url}/api/auth/callback/portal?ticket=<jwt>
# 生成测试票：pointer-server --mint-sso-ticket --sub USER_ID

[license]
key = "base64_payload.base64_sig"      # license key 字符串（非空时优先）
# 或：license_file = "license.key"     # 从文件读取；相对路径以本配置文件所在目录为基准

[usage]
report_enabled = false                 # standalone 默认不上报用量

# 模型、密钥、extra_body 在 Web 设置 → 模型配置中填写，不要写在本文件。

[server]
addr = "0.0.0.0:8787"
public_url = "https://pointer.example.com"
app_data_dir = "/var/lib/pointer"
# zip: skills beside binary; deb:
# static_dir = "/usr/share/pointer-server/dist"
# skills_dir = "/usr/share/pointer-server/skills"
static_dir = "dist"
skills_dir = "skills"

# --- Web 品牌文案（可选；详见 docs/ui/web-branding-welcome-elapsed.md）---
# page_title = "Acme · AI 助手"                 # env POINTER_SERVER_PAGE_TITLE
# composer_placeholder = "有什么可以帮你？"      # env POINTER_SERVER_COMPOSER_PLACEHOLDER
# welcome_tip_title = "我是 Acme AI 助手"        # env POINTER_SERVER_WELCOME_TIP_TITLE
# welcome_tip_body = "提交附件后我会自动处理…"    # env POINTER_SERVER_WELCOME_TIP_BODY
# turn_elapsed_active = "任务处理中"             # env POINTER_SERVER_TURN_ELAPSED_ACTIVE
# turn_elapsed_done = "任务已完成"               # env POINTER_SERVER_TURN_ELAPSED_DONE
# brand_name = "Acme 助手"                      # env POINTER_SERVER_BRAND_NAME
# brand_icon = "/branding/logo.png"            # env POINTER_SERVER_BRAND_ICON（左上/左下共用）
# desktop_snapshot_enabled = false             # env POINTER_SERVER_DESKTOP_SNAPSHOT_ENABLED
# （进行中收起条需在助手设置打开「默认收缩执行过程」，server 不强制）

# SSE 首帧 padding 注释帧（穿透缓冲型防火墙；默认关闭）
# sse_padding_enabled = false   # env POINTER_SERVER_SSE_PADDING_ENABLED
# sse_padding_bytes = 10240    # env POINTER_SERVER_SSE_PADDING_BYTES

# Browser CORS（默认关闭，仅同源）。桌面客户端走 IPC，不受影响。
# cors_origins = ["http://localhost:1420"]   # env POINTER_SERVER_CORS_ORIGINS

# 平台用户白名单（逗号分隔）。留空 = 允许任意平台用户。
# allowed_user_ids = ["1001", "1002"]         # env POINTER_SERVER_ALLOWED_USER_IDS
# 为 true 且白名单为空则拒绝启动
# require_allowed_users = false               # env POINTER_SERVER_REQUIRE_ALLOWED_USERS
# 禁止 Agent 终端里出现 SESSION_USER_ID 字面量（默认 false）
# forbid_session_user_id_in_terminal = false  # env POINTER_SERVER_FORBID_SESSION_USER_ID_IN_TERMINAL

# Webhook 入站鉴权（未设时 /api/webhooks/:src 一律 401）
[webhooks]
# bearer_token = "long-random-token"         # env POINTER_WEBHOOK_BEARER_TOKEN

# 任意环境变量注入：仅在该 KEY 尚未由 OS 环境变量设置时生效
[env]
# OTEL_EXPORTER_OTLP_ENDPOINT = "http://127.0.0.1:4317"

# 全局 MCP 服务（仅 TOML，无环境变量形式）
[[mcp_servers.server]]
# name = "weather"                           # 必填
# transport = "stdio"                        # stdio（默认）| http
# command = "npx"                            # stdio：启动命令
# args = ["-y", "mcp-weather"]
# url = "https://example.com/mcp"            # http：服务地址
# headers = { Authorization = "Bearer …" }   # http：附加请求头
```

> **`public_url` 不要带结尾 `/`。** 服务端用它拼回调地址（`{public_url}/api/auth/callback/<id>`），带尾斜杠会拼出 `//api/auth/callback/<id>`：路由匹配不到，浏览器拿到的是 SPA 首页 200，表现为「从门户点进来又回到登录页」，服务端日志里看不到回调请求。`pointer-server auth doctor` 会对尾斜杠给出警告。

### `[server]` Web 品牌 / 文案参数

| TOML | 环境变量 | 作用 | 未配置时 |
|------|----------|------|----------|
| `page_title` | `POINTER_SERVER_PAGE_TITLE` | 浏览器标签页 `<title>` | `Pointer · AI 工作台` |
| `composer_placeholder` | `POINTER_SERVER_COMPOSER_PLACEHOLDER` | 登录且可用后的输入框占位 | `告诉我你想做什么` |
| `welcome_tip_title` | `POINTER_SERVER_WELCOME_TIP_TITLE` | 全新空会话欢迎 tip 标题 | 不展示 tip |
| `welcome_tip_body` | `POINTER_SERVER_WELCOME_TIP_BODY` | 全新空会话欢迎 tip 正文 | 不展示 tip |
| `turn_elapsed_active` | `POINTER_SERVER_TURN_ELAPSED_ACTIVE` | 进行中回合耗时前缀 | `工作` |
| `turn_elapsed_done` | `POINTER_SERVER_TURN_ELAPSED_DONE` | 已结束回合耗时前缀 | `工作` |
| `brand_name` | `POINTER_SERVER_BRAND_NAME` | 侧栏/顶栏产品名 | `Pointer` |
| `brand_icon` | `POINTER_SERVER_BRAND_ICON` | 左上角与左下角共用 logo | `/app-icon.png` |
| `desktop_snapshot_enabled` | `POINTER_SERVER_DESKTOP_SNAPSHOT_ENABLED` | 桌面截图按钮 | 自动探测显示器 |
| `app_data_dir` | `POINTER_APP_DATA_DIR` | 运行时数据根目录 | OS 默认（与桌面 `PointerApp` 同源规则） |
| `sse_padding_enabled` | `POINTER_SERVER_SSE_PADDING_ENABLED` | SSE 首帧 padding | `false` |
| `sse_padding_bytes` | `POINTER_SERVER_SSE_PADDING_BYTES` | padding 字节数 | `10240` |
| `cors_origins` | `POINTER_SERVER_CORS_ORIGINS` | 浏览器 CORS 允许的 Origin | 关闭（仅同源） |

行为与前端约定见 [`../ui/web-branding-welcome-elapsed.md`](../ui/web-branding-welcome-elapsed.md)。本地 Vite 联调可用同名 `VITE_*` 覆盖 meta（空 `VITE_WEB_API_BASE` 走 `/api` 同源代理，避免跨站丢 cookie）。

### CORS

默认**关闭**：不挂 CORS 层，只服务同源浏览器请求。桌面客户端走 Tauri IPC，不受影响；pointer-server 同时托管静态 UI 时也是同源，无需开启。

| 配置 | 行为 |
|------|------|
| 省略 / 空 | 关闭 CORS |
| `cors_origins = ["*"]` 或 `POINTER_SERVER_CORS_ORIGINS=*` | 镜像任意 `Origin`，并允许带 cookie |
| `cors_origins = ["http://localhost:1420"]` | 精确白名单（`localhost` 与 `127.0.0.1` 不是同一个 Origin） |
| `"*"` 与具体 Origin 混写 | 启动失败 |

本地 `web:dev` 默认同源（Vite 把 `/api` 代理到 8787）。若前端仍直连 `http://127.0.0.1:8787`，需要打开 CORS。前后端分离部署（静态页与 API 不同源）同样需要配置允许的 Origin。

### 身份与访问控制（`[server]` / `[webhooks]`）

| TOML | 环境变量 | 默认 | 说明 |
|------|----------|------|------|
| `allowed_user_ids` | `POINTER_SERVER_ALLOWED_USER_IDS`（逗号分隔） | 空 | 空 = 允许任意平台用户；非空 = 精确白名单 |
| `require_allowed_users` | `POINTER_SERVER_REQUIRE_ALLOWED_USERS` | `false` | 为 true 且白名单为空 → **启动失败**（`POINTER_SERVER_REQUIRE_ALLOWED_USERS is set but POINTER_SERVER_ALLOWED_USER_IDS is empty`） |
| `forbid_session_user_id_in_terminal` | `POINTER_SERVER_FORBID_SESSION_USER_ID_IN_TERMINAL` | `false` | 为 true 时 Agent 的 `terminal` 拒绝 command / stdin 里出现 `SESSION_USER_ID` 字面量（软约束，防串号） |
| `[webhooks] bearer_token` | `POINTER_WEBHOOK_BEARER_TOKEN` | 空 | 未设时 `POST /api/webhooks/:src` 一律返回 401 |

> 对外可访问的实例建议同时设置 `public_url` 与 `allowed_user_ids`。白名单为空且公网可达，等于把实例交给任意平台用户。

### `[auth.providers.<id>]`（登录 provider）

所有登录方式都走这一节；登录页按 `GET /api/auth/providers` 的返回渲染，不存在“代码里写死的入口”。

| 字段 | 默认 | 说明 |
|------|------|------|
| `kind` | — | `platform` / `local_password` / `token_lookup` / `oauth2_userinfo` / `oidc` |
| `enabled` | 启用 | `false` 移除该 provider（含部署模式注入的默认项） |
| `display_name` | 按 kind | 登录页按钮文案 |
| `allow_in_managed` | `false` | managed 下启用 `local_password` 必须置 `true`，否则**启动失败** |
| `callback_path` | `/api/auth/callback/<id>` | `platform` 默认 `/api/auth/oauth/callback`（控制面白名单无需改动） |
| `icon` | 按 `kind` | 登录按钮图标名；显式配置优先，前端认识 `building` / `key` / `ticket` / `user` / `shield` / `globe`，其余回落为显示名首字母圆标 |

`icon` 的 `kind` 默认值：`platform` → `building`、`local_password` → `key`、`token_lookup` → `ticket`、`oauth2_userinfo` → `user`、`oidc` → `shield`。

`token_lookup` 的票据与字段映射：

| 字段 | 默认 | 说明 |
|------|------|------|
| `ticket_param` | `ticket` | 入口查询参数名 |
| `subject_path` | `/sub` | 稳定用户 id 的 claim |
| `nickname_path` | 空 | 可选显示名 claim |
| `signing.secret` / `signing.secret_prev` / `signing.audience` | 空 | 本地 HS256 验签；`audience` 须与票的 `aud` 一致 |
| `lookup.url` / `lookup.param` / `lookup.token` | 空 | 配了则改为不透明票回查 |

`oidc`（OpenID Connect：discovery 拿端点，JWKS 验签 ID token）：

| 字段 | 默认 | 说明 |
|------|------|------|
| `issuer`（provider 层，不是 `oidc` 子表） | — | **必填**，如 `https://sso.example.com/realms/acme`；discovery 由它推导 |
| `client_id` | — | **必填** |
| `client_secret` | — | **必填**；`auth = "none"`（公共客户端 + PKCE）时不需要 |
| `auth` | `client_secret_post` | token 请求的客户端认证：`client_secret_post` / `client_secret_basic` / `none` |
| `scopes` | 空 | 追加 scope；`openid` 永远排第一，重复项自动去掉 |
| `discovery_url` | `{issuer}/.well-known/openid-configuration` | 显式覆盖 discovery 文档地址 |
| `audience` | `client_id` | 校验 ID token 的 `aud` |
| `subject_path` | `/sub` | ID token 里稳定用户 id 的指针，不能为空 |
| `algs` | `RS256` | 接受的签名算法，仅 `RS256` / `RS384` / `RS512`，其余**启动失败** |
| `clock_skew_secs` | `120` | `exp` / `iat` 的时钟偏差容忍（秒） |

```toml
# 企业 OIDC（Keycloak 形状）：端点全部来自 discovery
[auth.providers.corp]
kind = "oidc"
display_name = "企业统一认证"
issuer = "https://sso.example.com/realms/acme"

[auth.providers.corp.oidc]
client_id = "pointer-web"
client_secret = "replace-with-client-secret"
scopes = ["profile", "email"]
# discovery_url 留空 → {issuer}/.well-known/openid-configuration
# audience 留空 → client_id；若 IdP 的 ID token 里 aud 不是 client_id，在这里显式指定
```

`oauth2_userinfo`（标准 OAuth2 授权码 + userinfo 取身份）：`authorize` / `token` / `userinfo` 三个子表，`url` 都必填 —— 这条路径没有 discovery。

| 字段（`authorize`） | 默认 | 说明 |
|------|------|------|
| `url` | — | **必填**，浏览器跳转的授权端点 |
| `scope` | 空 | 空格分隔，非空才发送 |
| `pkce` | `true` | 发送 `code_challenge`（S256） |
| `extra_params` | 空 | 静态附加查询参数（`audience` / `prompt` 等） |

| 字段（`token`） | 默认 | 说明 |
|------|------|------|
| `url` | — | **必填**，授权码换 token |
| `method` | `post` | `get` / `post` |
| `auth` | `client_secret_post` | `client_secret_post` / `client_secret_basic` / `none` |
| `client_id` / `client_secret` | — | `auth = "none"` 之外必填 |
| `encoding` | `json` | `post` 的 body 编码；**走 RFC 6749 标准端点要写 `form`** |
| `grant_type_param` / `grant_type_value` | `grant_type` / `authorization_code` | 参数名 / 参数值 |
| `code_param` | `code` | 不能为空 |
| `client_id_param` / `client_secret_param` | `client_id` / `client_secret` | 参数名可逐个覆盖 |
| `redirect_uri_param` / `code_verifier_param` | `redirect_uri` / `code_verifier` | 置 `""` 表示**不发送**该参数 |
| `extra_params` | 空 | 静态附加参数 |
| `access_token_path` | `/access_token` | 响应里 token 的指针 |
| `refresh_token_path` / `expires_in_path` / `id_token_path` | 空 | 可选响应字段指针 |
| `error` | `{ kind = "http_status" }` | 失败判定：`http_status`（非 2xx 即失败）/ `path_not_equal` / `path_present` |
| `error_code_path` / `error_message_path` | 空 | 可选错误字段指针 |

| 字段（`userinfo`） | 默认 | 说明 |
|------|------|------|
| `url` | — | **必填** |
| `method` | `get` | `get` / `post` |
| `token_auth` | `header` | `header` / `query` / `body`；`body` 必须配 `method = "post"`，否则**启动失败** |
| `token_name` | `header` → `Authorization`；`query` / `body` → `access_token` | 承载 token 的字段名 |
| `token_prefix` | `header` → `Bearer `；其余为空 | 置 `""` 表示发**裸 token**（如 `x-acs-dingtalk-access-token`） |
| `encoding` | `form` | 只在 `token_auth = "body"` 时生效 |
| `subject_path` | `/sub` | 稳定用户 id，不能为空 |
| `issuer_path` | 空 | 可选 issuer claim 指针；留空用 provider 层 `issuer` |
| `error` / `error_code_path` / `error_message_path` | 同 `token` | 失败判定与错误字段 |

```toml
# 标准 OAuth2 + userinfo（RFC 6749 形状）
[auth.providers.corp-oauth]
kind = "oauth2_userinfo"
display_name = "企业账号登录"
issuer = "https://idp.example.com"

[auth.providers.corp-oauth.authorize]
url = "https://idp.example.com/oauth2/authorize"
scope = "openid profile"

[auth.providers.corp-oauth.token]
url = "https://idp.example.com/oauth2/token"
client_id = "pointer-web"
client_secret = "replace-with-client-secret"
encoding = "form"                 # 默认 json 只适合少数非标准实现

[auth.providers.corp-oauth.userinfo]
url = "https://idp.example.com/oauth2/userinfo"
subject_path = "/sub"
```

国内实现常不按 RFC 6749 来，靠字段微调即可，不用改代码：`token.auth = "none"` 表示公共客户端；`token.extra_params` 补固定参数；`userinfo.token_name = "x-acs-dingtalk-access-token"` + `userinfo.token_prefix = ""` 发裸 token；`userinfo.subject_path = "/unionId"` 取厂商自己的稳定 id；失败判定不是非 2xx 时用 `error = { kind = "path_not_equal", path = "/errcode", ok_value = "0" }`。

> 回调地址由 `public_url` 拼出：**不要给 `public_url` 加结尾 `/`**，否则得到 `//api/auth/callback/<id>`，路由匹配不到，表现为「从门户点进来又回到登录页」，服务端日志里看不到回调请求。

准入白名单：`local_password` 豁免，其余 kind 均过 `server_access::is_user_allowed`。

登录流程见 [`standalone-local-login.md`](standalone-local-login.md)；运维视角见 [`../user/standalone-server.md`](../user/standalone-server.md)。

### `[auth.rate_limit]`（登录入口限流）

默认**开启**，保护 `POST /api/auth/login/start` 与 `POST /api/auth/login/finish` 免遭爆破与洪水。计数在**进程内存**里：单实例有效，重启清零。只走 TOML，没有环境变量形式。

| TOML | 默认 | 说明 |
|------|------|------|
| `enabled` | `true` | 总开关；`false` 时按 IP 窗口、在途上限与失败冷却全部关闭 |
| `per_ip_per_minute` | `20` | 每 IP 每分钟允许的 `login/start` 次数；`0` = 不按 IP 限 |
| `max_pending` | `256` | 同时在途的登录上限；`0` = 不限 |
| `finish_fail_backoff_secs` | `3` | `login/finish` 失败（`401 invalid_captcha` / `401 invalid_credentials`）后的冷却秒数，成功即清除；`0` = 关闭 |
| `trusted_proxies` | 空 | 采信其 `X-Forwarded-For` 的代理地址（精确 IP 或 CIDR） |

被拒时一律 **429**，body 是纯文本错误码：

| body | 含义 |
|------|------|
| `rate_limited` | 该 IP 本分钟 `login/start` 次数超过 `per_ip_per_minute` |
| `too_many_attempts` | 该 IP 处于 `finish_fail_backoff_secs` 冷却中 |
| `login_capacity_reached` | 在途登录已达 `max_pending` |

超过 `max_pending` 是**拒绝新请求**，绝不淘汰进行中的登录 —— 淘汰会让那个用户拿到莫名的 `invalid_state`。

只有**直连对端 IP 命中 `trusted_proxies`** 时才读 `X-Forwarded-For`，并取最右侧非可信跳作为客户端 IP，客户端自己伪造左边条目无效；不在列表里的请求一律按 socket 地址计数。反向代理后面部署时把代理地址（精确 IP，或 `10.0.0.0/8` 这类 CIDR）列进去，否则所有用户共用代理 IP 的额度。

### `[env]` 与 `[[mcp_servers.server]]`

`[env]` 是**任意键值注入**：键名就是环境变量名。唯一规则是**仅在该环境变量尚未由 OS 设置时生效** —— OS 环境变量永远优先，注入只补空缺。路径类键（后缀 `_DIR` / `_PATH`、等于 `PATH`、含 `STATIC`）的相对值按配置文件所在目录解析。

`[[mcp_servers.server]]` 声明**全局 MCP 服务**，只走 TOML（没有环境变量形式）：

| 字段 | 必填 | 默认 | 说明 |
|------|------|------|------|
| `name` | ✅ | — | 服务名 |
| `transport` | — | `stdio` | `stdio` 或 `http` |
| `command` / `args` / `env` | — | — | `stdio` 用 |
| `url` / `headers` | — | — | `http` 用 |

### 配置文件发现顺序

```
1. POINTER_SERVER_CONFIG             ← 显式路径；设了但不可读 → 直接报错，不回退
2. {exe_dir}/pointer-server.toml
3. {exe_dir}/pointer-server.env
4. {cwd}/pointer-server.toml
5. {cwd}/pointer-server.env
```

**第一个存在的文件胜出，只加载一个，不做合并。** 文件里的键会被映射成环境变量注入进程，但 **OS 环境变量永远优先**：已经由 OS 设好的键会被跳过。启动日志会打印 applied / skipped 清单（敏感键脱敏）。

> 第二个候选扩展名是 `pointer-server.env`，不是 `.env`。

---



## 改造文件清单



### 新增文件（9 个）


| 文件                                           | 说明                                          |
| -------------------------------------------- | ------------------------------------------- |
| `crates/pointer-core/src/deployment_mode.rs` | 部署模式检测：`platform` / `standalone`            |
| `crates/pointer-core/src/local_auth.rs`      | 账号密码 HMAC 本地认证                              |
| `crates/pointer-core/src/license/mod.rs`     | License 模块入口                                |
| `crates/pointer-core/src/license/verify.rs`  | Ed25519 验签、启动校验、热加载、机器绑定                    |
| `crates/pointer-core/license.pub`            | 编译嵌入的公钥文件                                   |
| `server/src/auth_routes.rs`                  | 统一登录入口与回调；`local_password` 走 `login/start` + `login/finish` |
| `tools/license-gen/Cargo.toml`               | License 签发 CLI                              |
| `tools/license-gen/src/main.rs`              | `gen-keypair` / `sign` 子命令                  |
| `tools/license-gen/dev-license.key.example`  | 开发用私钥示例                                     |




### 修改文件（12 个）


| 文件                                                      | 改动                                                     |
| ------------------------------------------------------- | ------------------------------------------------------ |
| `Cargo.toml`（workspace）                                 | 加入 `tools/license-gen`                                 |
| `crates/pointer-core/Cargo.toml`                        | 加 `ed25519-dalek` 依赖                                   |
| `crates/pointer-core/build.rs`                          | 编译嵌入 `license.pub`                                     |
| `crates/pointer-core/src/lib.rs`                        | 注册 3 个新模块                                              |
| `crates/pointer-core/src/server_config.rs`              | 解析 `[deployment]`、`[auth.local]`、`[license]` 段 |
| `crates/pointer-core/src/platform_endpoints.rs`         | standalone 不回落 readflowai                              |
| `crates/pointer-core/src/web_request_auth.rs`           | `auth_kind` 字段                                         |
| `crates/pointer-core/src/cloud_agent_auth.rs`           | standalone 禁用云 OAuth                                   |
| `crates/pointer-core/src/token_usage_store.rs`          | standalone 不上报                                         |
| `crates/pointer-core/src/chat_service/session_inner.rs` | 本地 session 跳过平台检查                                      |
| `server/src/web_session.rs`                             | session 携带 `auth_kind`                                 |
| `server/src/main.rs`                                    | 启动流程 + `--machine-id` + 路由 + 鉴权分支                      |


---



## 编译与构建



### 二进制

```bash
cargo build -p pointer-server --release
```



### zip 包（跨平台，自动）

```bash
npm run server:build
# macOS / Windows → zip
# Linux         → zip + .deb（有 dpkg-deb 时）
```



### .deb 包（Linux，已包含在 server:build 中）

单独重打 deb（需已有 release 二进制）：

```bash
npm run server:build:deb
# 产物：target/release/bundle/deb/pointer-server_0.1.0_*.deb
```



### 单元测试

```bash
cargo test -p pointer-core -- license::verify
```



### License 签发 CLI

```bash
npm run license-gen:build
# 产物：target/release/license-gen-bundle/license-gen-{platform}-{arch}.zip
```

开发调试：

```bash
npm run license-gen:dev -- sign --help
```

