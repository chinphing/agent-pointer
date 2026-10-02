# 独立部署开发文档

[English](../../en/developer/standalone-deployment.md) | 简体中文

pointer-server 支持**脱离官方平台独立部署**。本文档覆盖架构、配置格式、API 端点与实现模块。

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

**文件：** `crates/pointer-core/src/local_auth.rs`、`server/src/local_auth.rs`

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
GET  /api/auth/mode              → { "mode": "standalone" | "platform" }
GET  /api/auth/local/captcha     → { captchaId, imageSvg }  （内存一次性，TTL 5min）
POST /api/auth/local/login
  { username, password, captchaId, captcha }
  → 校验验证码 → verify_local_password
  → 成功：创建 WebSession（auth_kind = Local）+ Set-Cookie
  → 失败：401 invalid_captcha | invalid_credentials
```

**Session：** `WebSessionAuthKind::Local`；所有需登录 API 在 standalone 下要求本地会话。

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
| `agents/computer/vision/annotate.rs` | Computer Agent 标注能力降级（需自建 `COMPUTER_ANNOTATE_API_BASE`） |


---



## API 端点


| 端点                        | 方法   | 说明                                                         |
| ------------------------- | ---- | ---------------------------------------------------------- |
| `/api/auth/mode`          | GET  | `{ "mode": "standalone" | "platform" }`                    |
| `/api/auth/local/captcha` | GET  | `{ captchaId, imageSvg }`，standalone only                  |
| `/api/auth/local/login`   | POST | `{ username, password, captchaId, captcha }`，返回 Set-Cookie |
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

# 第三方 SSO 短时票（可选；不配则只用账号密码）
# [auth.local.sso]
# enabled = true                       # env POINTER_SERVER_SSO_ENABLED（不设则按 secret / audience 自动判定）
# secret = "shared-with-portal"        # env POINTER_SERVER_SSO_SECRET
# secret_prev = "rotating-old-secret"  # env POINTER_SERVER_SSO_SECRET_PREV（轮换窗口用的次密钥）
# audience = "pointer-server"          # env POINTER_SERVER_SSO_AUDIENCE（须与票的 aud 一致）
# max_skew_secs = 30                   # env POINTER_SERVER_SSO_MAX_SKEW_SECS（默认 30）

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

### `[auth.local.sso]`（第三方短时票）

| TOML | 环境变量 | 默认 |
|------|----------|------|
| `enabled` | `POINTER_SERVER_SSO_ENABLED` | 不设时按 `secret` / `audience` 是否配置自动判定 |
| `secret` | `POINTER_SERVER_SSO_SECRET` | 空 |
| `secret_prev` | `POINTER_SERVER_SSO_SECRET_PREV` | 空（轮换窗口用的次密钥） |
| `audience` | `POINTER_SERVER_SSO_AUDIENCE` | 空（须与票的 `aud` 一致） |
| `max_skew_secs` | `POINTER_SERVER_SSO_MAX_SKEW_SECS` | `30` |

登录流程见 [`standalone-local-login.md`](standalone-local-login.md)；运维视角见 [`../user/standalone-server.md`](../user/standalone-server.md)。

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
| `server/src/local_auth.rs`                   | `/api/auth/local/login`、`/api/license/*` 路由 |
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

