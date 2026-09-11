# 独立部署开发文档

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
| `platform`（默认） | 连接 [readflowai.com](https://pointer-api.readflowai.com)，使用官方 OAuth + 云端 LLM Key 下发 |
| `standalone`   | 脱离官方平台，使用本地账号密码登录 + Web 设置配置模型/密钥 + Ed25519 License 校验                           |


**余额 / LLM 门禁：** 官方账户余额校验（`ensure_llm_allowed`、`GET /auth/partner/llm-credentials`）仅在 `platform` 模式生效；`standalone` 下为 no-op，不访问官方余额 API。

```rust
pub fn deployment_mode() -> Mode;
pub fn is_standalone() -> bool;
```

配置来源优先级：

1. `POINTER_DEPLOYMENT_MODE` 环境变量（`platform` / `standalone`）
2. `[deployment].mode` 配置文件值

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
  → deployment_mode == standalone?
    → 读取 POINTER_LICENSE_KEY / [license].key
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

Standalone **不从** `pointer-server.toml` 读取模型、地址或 API Key。配置文件里的 `[llm]` 段会被忽略。

登录后打开 **设置 → 模型配置 → 自定义服务**，与桌面客户端自定义服务相同：添加服务商（id、名称、API 地址、模型名单、密钥），以及上下文、最大输出、思考强度、能力勾选、`extra_body` 和单模型覆盖。保存走 `updateUserSettings`，写入 `user_settings.json`；密钥以 `enc:v1:` 加密落盘。

本地默认不再内置任何平台服务商/模型（`activeProviderId` / `model` 默认为空）。未在界面添加服务并填写密钥时，对话不可用。

---



## 平台副作用管理


| 模块                                   | Standalone 行为                                           |
| ------------------------------------ | ------------------------------------------------------- |
| `platform_endpoints.rs`              | 不回落 readflowai.com 默认域名，未配置时 warn                       |
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

完整示例见 `[server/pointer-server.toml.example](../../server/pointer-server.toml.example)`。

```toml
[deployment]
mode = "standalone"                    # platform（默认）| standalone

[auth.local]
username = "admin"
hmac_secret = "replace-with-long-random-secret"
# pointer-server --hash-password --secret '<hmac_secret>' '<password>'
password_hmac = "...."

[license]
key = "base64_payload.base64_sig"      # license key 字符串
# 或：key_file = "license.key"         # 从文件读取

[usage]
report_enabled = false                 # standalone 默认不上报用量

# 模型、密钥、extra_body 在 Web 设置 → 模型配置中填写，不要写在本文件。

[server]
addr = "0.0.0.0:8787"
public_url = "https://pointer.example.com"
# page_title = "Acme · AI 助手"   # browser tab; env POINTER_SERVER_PAGE_TITLE
# composer_placeholder = "有什么可以帮你？"  # composer hint; env POINTER_SERVER_COMPOSER_PLACEHOLDER
# welcome_tip_title / welcome_tip_body  # brand-new empty chat tip; env POINTER_SERVER_WELCOME_TIP_*
# turn_elapsed_active / turn_elapsed_done  # elapsed chip prefixes; env POINTER_SERVER_TURN_ELAPSED_*
# （进行中收起条需在助手设置打开「默认收缩执行过程」，server 不强制）
# SSE 首帧 padding 注释帧（穿透缓冲型防火墙；默认关闭，需要时显式开启 sse_padding_enabled = true）
# sse_padding_enabled = false   # env POINTER_SERVER_SSE_PADDING_ENABLED
# sse_padding_bytes = 10240    # env POINTER_SERVER_SSE_PADDING_BYTES
app_data_dir = "/var/lib/pointer"
# zip: skills beside binary; deb:
# static_dir = "/usr/share/pointer-server/dist"
# skills_dir = "/usr/share/pointer-server/skills"
static_dir = "dist"
skills_dir = "skills"
```

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

