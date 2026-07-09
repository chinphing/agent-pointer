# 独立部署开发文档

pointer-server 支持**脱离官方平台独立部署**。本文档覆盖架构、配置格式、API 端点与实现模块。

## 目录

- [部署模式（deployment_mode.rs）](#部署模式)
- [本地认证（local_auth.rs）](#本地认证)
- [License 系统（license/）](#license-系统)
- [LLM Provider 注入](#llm-provider-注入)
- [API 端点](#api-端点)
- [配置参考](#配置参考)
- [改造文件清单](#改造文件清单)

---

## 部署模式

**文件：** `crates/pointer-core/src/deployment_mode.rs`

| 模式 | 说明 |
|------|------|
| `platform`（默认） | 连接 [readflowai.com](https://pointer-api.readflowai.com)，使用官方 OAuth + 云端 LLM Key 下发 |
| `standalone` | 脱离官方平台，使用本地 Admin Token 登录 + TOML 注入 LLM Key + Ed25519 License 校验 |

```rust
pub fn deployment_mode() -> Mode;
pub fn is_standalone() -> bool;
```

配置来源优先级：
1. `POINTER_DEPLOYMENT_MODE` 环境变量（`platform` / `standalone`）
2. `[deployment].mode` 配置文件值

---

## 本地认证

**文件：** `crates/pointer-core/src/local_auth.rs`

Standalone 模式下替代官方 OAuth 的认证机制。提供 Admin Token 校验和本地 Session 创建。

```rust
pub struct LocalAuthConfig {
    pub admin_token: String,
    pub enabled: bool,
}
pub fn load_local_auth_config() -> LocalAuthConfig;
pub fn verify_admin_token(token: &str) -> bool;
```

**流程：**
```
POST /api/auth/local/login  { "token": "..." }
  → verify_admin_token(token)
  → 成功：创建 WebSession（auth_kind = "local"）
  → 失败：401
```

**Session 扩展：** `WebSessionEntry` 增加 `auth_kind: String` 字段（`"platform"` / `"local"`），所有需要登录的 API 根据部署模式检查对应的 session 类型。

### Standalone 模式下 LLM 凭证路径

在 `chat_service/session_inner.rs` 中：
- standalone + web_session + `has_local_llm` → 跳过 `ensure_llm_allowed()`（无平台 quota 检查）
- standalone + 无本地 LLM Key → 报错提示配置 LLM

---

## License 系统

**文件：** `crates/pointer-core/src/license/mod.rs` + `src/license/verify.rs`

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
  "machine_id": "5A372B48-..."
}
```

### 模块结构

```rust
pub struct LicenseClaims { pub customer_id, pub expires_at, pub features, pub max_seats, pub machine_id }
pub enum LicenseStatus { Valid, Expired, NotConfigured, Invalid, MachineMismatch }
pub struct LicenseVerifier { public_key: VerifyingKey }

// 核心函数
pub fn validate_license_at_startup() -> Result<()>       // main() 启动时调用
pub fn reload_license_from_env() -> Result<LicenseStatusView>  // 热加载
pub fn active_license_claims() -> Option<LicenseClaims>
pub fn active_license_status_view() -> LicenseStatusView
pub fn feature_enabled(claims: &LicenseClaims, feature: &str) -> bool
pub fn current_machine_id() -> anyhow::Result<String>     // 读取硬件 ID
```

### 验证流程

```
main()
  → deployment_mode == standalone?
    → 读取 POINTER_LICENSE_KEY / [license].key
    → base64url 解码 payload 和 signature
    → Ed25519 验签（公钥编译嵌入 license.pub, 可被 POINTER_LICENSE_PUBLIC_KEY 覆盖）
    → 检查 expiry
    → 如果 claims.machine_id 不为空，校验机器 ID 是否匹配
    → 缓存 claims → 运行
```

### 机器绑定

利用 [`machine-uid`]((https://crates.io/crates/machine-uid)) crate 读取硬件标识：

| 平台 | 来源 |
|------|------|
| macOS | IOPlatformUUID（`ioreg -rd1 -c IOPlatformExpertDevice`） |
| Linux | `/etc/machine-id` |
| Windows | `HKEY_LOCAL_MACHINE\SOFTWARE\Microsoft\Cryptography\MachineGuid` |

通过 `--machine-id` 标志获取（无需任何配置）：
```bash
./pointer-server --machine-id
# 输出：5A372B48-8721-5807-9645-8E7A560F2518
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

# 签发（绑定当前机器）
cargo run -p pointer-license-gen -- sign \
  --private-key license.key \
  --customer-id acme \
  --expires 2027-12-31 \
  --features chat,webhook \
  --bind-machine

# 签发（绑定指定机器 ID）
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

## LLM Provider 注入

Standalone 模式下从 TOML 配置注入 LLM Key，替代 OAuth 下发。

**配置格式（`pointer-server.toml`）：**
```toml
[llm]
active_provider = "qwen"

[llm.providers.qwen]
api_key = "sk-..."
base_url = "https://dashscope.aliyuncs.com/compatible-mode/v1"
name = "千问"
models = ["qwen3.5-plus", "qwen3.5-turbo"]

[llm.providers.glm]
api_key = "sk-..."
base_url = "https://open.bigmodel.cn/api/paas/v4"
name = "智谱 GLM"
models = ["glm-5", "glm-5-flash"]
```

**实现：** `server_config.rs` → `apply_llm_providers_from_config()`，启动时写入 `platform_config`（与 OAuth 注入走同一路径）。

**支持的 provider id：** `qwen`, `deepseek`, `glm`, `kimi`, `openai-compatible`

---

## 平台副作用管理

| 模块 | Standalone 行为 |
|------|----------------|
| `platform_endpoints.rs` | 不回落 readflowai.com 默认域名，未配置时 warn |
| `cloud_agent_auth.rs` | 禁用云 OAuth code exchange |
| `token_usage_store.rs` | `usage_report_enabled()` 默认 false，跳过上报 |
| `media/oss.rs` | 从 `[media_oss]` 或 `OSS_*` 环境变量读取，不从 OAuth 注入 |
| `experiences.rs` | 直连 `pointer-api` 失败时降级（欢迎页不可用） |
| `agents/computer/vision/annotate.rs` | Computer Agent 标注能力降级（需自建 `COMPUTER_ANNOTATE_API_BASE`） |

---

## API 端点

| 端点 | 方法 | 说明 |
|------|------|------|
| `/api/auth/local/login` | POST | body `{"token": "..."}`，返回 Set-Cookie |
| `/api/license/status` | GET | 返回 license claims、状态、机器绑定信息 |
| `/api/license/reload` | POST | 从 `POINTER_LICENSE_KEY` 热加载新 license |

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

完整示例见 [`server/pointer-server.toml.example`](../../server/pointer-server.toml.example)。

```toml
[deployment]
mode = "standalone"                    # platform（默认）| standalone

[auth.local]
admin_token = "your-strong-secret"     # standalone 管理员登录令牌

[license]
key = "base64_payload.base64_sig"      # license key 字符串
# 或：key_file = "license.key"         # 从文件读取

[usage]
report_enabled = false                 # standalone 默认不上报用量

[llm]
active_provider = "qwen"               # 默认使用的 LLM provider

[llm.providers.qwen]
api_key = "sk-..."
base_url = "https://dashscope.aliyuncs.com/compatible-mode/v1"
name = "千问"
models = ["qwen3.5-plus"]

[server]
addr = "0.0.0.0:8787"
public_url = "https://pointer.example.com"
app_data_dir = "/var/lib/pointer"
```

---

## 改造文件清单

### 新增文件（9 个）

| 文件 | 说明 |
|------|------|
| `crates/pointer-core/src/deployment_mode.rs` | 部署模式检测：`platform` / `standalone` |
| `crates/pointer-core/src/local_auth.rs` | Admin Token 本地认证 |
| `crates/pointer-core/src/license/mod.rs` | License 模块入口 |
| `crates/pointer-core/src/license/verify.rs` | Ed25519 验签、启动校验、热加载、机器绑定 |
| `crates/pointer-core/license.pub` | 编译嵌入的公钥文件 |
| `server/src/local_auth.rs` | `/api/auth/local/login`、`/api/license/*` 路由 |
| `tools/license-gen/Cargo.toml` | License 签发 CLI |
| `tools/license-gen/src/main.rs` | `gen-keypair` / `sign` 子命令 |
| `tools/license-gen/dev-license.key.example` | 开发用私钥示例 |

### 修改文件（12 个）

| 文件 | 改动 |
|------|------|
| `Cargo.toml`（workspace） | 加入 `tools/license-gen` |
| `crates/pointer-core/Cargo.toml` | 加 `ed25519-dalek` 依赖 |
| `crates/pointer-core/build.rs` | 编译嵌入 `license.pub` |
| `crates/pointer-core/src/lib.rs` | 注册 3 个新模块 |
| `crates/pointer-core/src/server_config.rs` | 解析 `[deployment]`、`[auth.local]`、`[llm]`、`[license]` 段 |
| `crates/pointer-core/src/platform_endpoints.rs` | standalone 不回落 readflowai |
| `crates/pointer-core/src/web_request_auth.rs` | `auth_kind` 字段 |
| `crates/pointer-core/src/cloud_agent_auth.rs` | standalone 禁用云 OAuth |
| `crates/pointer-core/src/token_usage_store.rs` | standalone 不上报 |
| `crates/pointer-core/src/chat_service/session_inner.rs` | 本地 session 跳过平台检查 |
| `server/src/web_session.rs` | session 携带 `auth_kind` |
| `server/src/main.rs` | 启动流程 + `--machine-id` + 路由 + 鉴权分支 |

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
