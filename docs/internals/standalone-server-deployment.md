# pointer-server 独立部署完整流程（内部）

面向 Pointer 团队：**从构建发布包、签发 License，到客户侧 standalone 上线** 的一站式操作手册。

用户向导读 [`../user/standalone-server.md`](../user/standalone-server.md)；实现细节见 [`../developer/standalone-deployment.md`](../developer/standalone-deployment.md)。

---

## 1. 角色与交付物

| 角色 | 职责 | 交付物 |
|------|------|--------|
| **Pointer 构建机** | 编译 server / license-gen | zip 或 deb 安装包 |
| **Pointer 签发方** | 保管 Ed25519 私钥，为客户签 License | `license.key`（离线）、签发的 License 字符串 |
| **客户运维** | 部署 server、配置 LLM、账号密码 | 可访问的 Web UI + API |

Standalone 模式要点：

- **必须**有效 License（Ed25519 验签，公钥编译在 `crates/pointer-core/license.pub`）
- **必须**在 TOML 配置 LLM Provider（各供应商 `api_key` 由客户自行填写）
- **使用账号密码 + 验证码登录**，不走 readflowai.com OAuth
- 默认**不上报** Token 用量（`report_enabled = false`）

---

## 2. 构建发布包（Pointer 侧）

统一命令（与桌面端 `tauri:build` 相同 `{模块}:dev|build` 风格）：

```bash
# 开发调试
npm run server:dev

# 打包（自动按当前 OS 选择产物）
npm run server:build
```

| 构建机 OS | 产物 |
|-----------|------|
| macOS | `target/release/pointer-server-bundle/pointer-server-macos-{arm64\|x64}.zip` |
| Windows | `target/release/pointer-server-bundle/pointer-server-windows-x64.zip` |
| Linux | 上述 zip + `target/release/bundle/deb/pointer-server_0.1.0_{amd64\|arm64}.deb`（需 `dpkg-deb`） |

zip 包内目录结构：

```text
pointer-server/
├── pointer-server              # Linux/macOS 二进制（Windows 为 pointer-server.exe）
├── dist/                       # Vue Web UI（同域托管）
├── skills/                     # 内置默认 Skills
├── pointer-server.toml.example
├── start.sh / stop.sh / restart.sh / status.sh
└── start.ps1 / stop.ps1 / restart.ps1 / status.ps1
```

仅重打 zip（已有 release 二进制）：

```bash
npm run server:package
```

License 签发工具打包：

```bash
npm run license-gen:build
# → target/release/license-gen-bundle/license-gen-{platform}-{arch}.zip
```

---

## 3. License 密钥体系（首次，Pointer 签发方）

### 3.1 生成 Ed25519 密钥对

**生产私钥严格离线保存，禁止进 Git。**

```bash
npm run license-gen:build
# 或开发机直接：
cargo run -p pointer-license-gen -- gen-keypair \
  --private-key /secure/offline/license.key \
  --public-key crates/pointer-core/license.pub
```

| 文件 | 用途 |
|------|------|
| `license.key` | 离线签发私钥（仅 license-gen 机器） |
| `crates/pointer-core/license.pub` | 验签公钥，**编译进 pointer-server** |

更新公钥后需**重新编译并发布** pointer-server，客户旧 License 若密钥轮换则失效。

开发环境可使用仓库内测试向量说明：`tools/license-gen/dev-license.key.example`（**禁止用于生产**）。

---

## 4. 为客户签发 License

### 4.1 收集客户机器绑定信息

在**目标部署机器**执行：

```bash
./pointer-server --machine-id          # 主绑定 token（fp1:…）
./pointer-server --machine-id-json     # 完整 JSON（推荐远程签发）
```

**v2 指纹（P1/P2）**

- `machineId` = `fp1:` + SHA256（salt + os_id + board_uuid + cloud_instance）
- `machineBoardFp` / `machineCloudFp` = 漂移锚点：OS 重装后 strict 变化，board 或 cloud 仍匹配则通过
- 云 VM 通过 AWS/Azure IMDS（`169.254.169.254`）读取 instance id
- **旧 License**（裸 os id 字符串）仍兼容 exact match

| 平台 | os_id | board | cloud |
|------|-------|-------|-------|
| Linux | `/etc/machine-id` | DMI product_uuid | AWS/Azure IMDS |
| macOS | IOPlatformUUID | IOPlatformUUID | — |
| Windows | MachineGuid | WMI BIOS UUID | — |

### 4.2 签发命令

**绑定指定机器（推荐生产，含漂移锚点）：**

```bash
# 客户发来 identity.json（来自 --machine-id-json）
./license-gen sign \
  --private-key /secure/offline/license.key \
  --customer-id acme-corp \
  --expires 2027-12-31 \
  --features chat,webhook,channels \
  --machine-id-json ./identity.json
```

**在签发机本机绑定（`--bind-machine`）：**

```bash
./license-gen sign \
  --private-key /secure/offline/license.key \
  --customer-id acme-corp \
  --expires 2027-12-31 \
  --features chat,webhook,channels \
  --bind-machine
```

**仅 strict token（`--machine-id fp1:…`）：**

```bash
./license-gen sign \
  --private-key /secure/offline/license.key \
  --customer-id acme-corp \
  --expires 2027-12-31 \
  --features chat,webhook,channels \
  --machine-id "fp1:…"
```

**Legacy 裸 os id（旧版兼容，无漂移）：**

```bash
./license-gen sign \
  --private-key /secure/offline/license.key \
  --customer-id acme-corp \
  --expires 2027-12-31 \
  --features chat,webhook,channels \
  --machine-id "5A372B48-8721-5807-9645-8E7A560F2518"
```

**不绑机器（仅测试，慎用）：**

```bash
./license-gen sign \
  --private-key /secure/offline/license.key \
  --customer-id acme-corp \
  --expires 2027-12-31 \
  --features chat,webhook,channels
```

stdout 输出一行 License Key，格式：`base64url(payload).base64url(signature)`

License payload 字段：

| 字段 | 说明 |
|------|------|
| `customer_id` | 客户标识，如 `acme-corp` |
| `expires_at` | Unix 时间戳（`--expires` 支持 `YYYY-MM-DD` 或 RFC3339） |
| `features` | 功能列表，常用：`chat`, `webhook`, `channels` |
| `max_seats` | 可选，最大用户数 |
| `machine_id` | 可选，主绑定 token（`fp1:…` 或 legacy os id） |
| `machine_board_fp` | 可选，board 漂移锚点（v2） |
| `machine_cloud_fp` | 可选，cloud instance 漂移锚点（v2） |

---

## 5. 客户侧部署

### 5.1 方式 A：zip 包（macOS / Windows / Linux 通用）

```bash
unzip pointer-server-linux-amd64.zip
cd pointer-server
cp pointer-server.toml.example pointer-server.toml
# 编辑 pointer-server.toml（见第 6 节完整模板）
./start.sh          # Linux / macOS
# .\start.ps1       # Windows
```

### 5.2 方式 B：deb 包（Linux 推荐）

```bash
sudo dpkg -i pointer-server_0.1.0_amd64.deb
/usr/bin/pointer-server --machine-id
sudo cp /etc/pointer-server/pointer-server.toml.example /etc/pointer-server/pointer-server.toml
sudo vi /etc/pointer-server/pointer-server.toml
sudo systemctl enable --now pointer-server
```

deb 安装路径：

| 路径 | 内容 |
|------|------|
| `/usr/bin/pointer-server` | 二进制 |
| `/usr/share/pointer-server/dist/` | Web UI |
| `/usr/share/pointer-server/skills/` | 内置 Skills |
| `/etc/pointer-server/pointer-server.toml` | 运行时配置 |
| `/var/lib/pointer-server/` | 数据目录（deb systemd 默认） |
| `/var/log/pointer-server/` | postinst 创建的日志目录 |

systemd 单元内置环境变量：

```ini
Environment=POINTER_DEPLOYMENT_MODE=standalone
Environment=POINTER_APP_DATA_DIR=/var/lib/pointer-server
Environment=POINTER_SERVER_CONFIG=/etc/pointer-server/pointer-server.toml
Environment=POINTER_SERVER_STATIC_DIR=/usr/share/pointer-server/dist
Environment=POINTER_SERVER_SKILLS_DIR=/usr/share/pointer-server/skills
```

启动时会把 `POINTER_SERVER_SKILLS_DIR`（或 zip 旁的 `skills/`）同步到 `{POINTER_APP_DATA_DIR}/skills/`，再载入 Skill 目录。若日志出现 `bundled skills: no source directory found`，说明未找到内置 Skills 源目录。
---

## 6. 完整配置模板（`pointer-server.toml`）

以下模板包含 standalone 所需的**全部默认段**；**仅 `api_key` 留空由客户填写**，其余使用推荐默认值。

配置文件查找顺序：

1. 环境变量 `POINTER_SERVER_CONFIG` 指向的绝对路径
2. 可执行文件同目录的 `pointer-server.toml`

```toml
# =============================================================================
# pointer-server standalone 生产配置模板
# 复制为 pointer-server.toml 后修改 api_key、auth.local、license.key、public_url
# =============================================================================

[deployment]
mode = "standalone"

[auth.local]
username = "admin"
hmac_secret = "REPLACE-WITH-LONG-RANDOM-SECRET"
# Generate: pointer-server --hash-password --secret '<hmac_secret>' '<password>'
password_hmac = "REPLACE-WITH-HMAC-HEX"

[license]
# Pointer 签发的 License Key（单行，payload.signature）
key = "REPLACE-WITH-LICENSE-KEY-FROM-POINTER"
# 或从文件读取：
# license_file = "/etc/pointer-server/license.key"

[usage]
report_enabled = false

[llm]
active_provider = "qwen"

# --- 阿里云百炼 DashScope（千问，默认主 Provider）---
# 控制台：https://bailian.console.aliyun.com/
[llm.providers.qwen]
api_key = "sk-REPLACE-ME"
base_url = "https://dashscope.aliyuncs.com/compatible-mode/v1"
name = "千问"
models = ["qwen3.5-plus", "qwen3.5-turbo", "qwen3.5-flash"]

# --- DeepSeek 深度求索 ---
# 控制台：https://platform.deepseek.com/
[llm.providers.deepseek]
api_key = "sk-REPLACE-ME"
base_url = "https://api.deepseek.com/v1"
name = "深度求索"
models = ["deepseek-v4-pro", "deepseek-v4-flash"]

# --- 智谱 AI GLM ---
# 控制台：https://open.bigmodel.cn/
[llm.providers.glm]
api_key = "sk-REPLACE-ME"
base_url = "https://open.bigmodel.cn/api/paas/v4"
name = "智谱 GLM"
models = ["glm-5", "glm-5-flash"]

# --- 月之暗面 Moonshot Kimi ---
# 控制台：https://platform.moonshot.cn/
[llm.providers.kimi]
api_key = "sk-REPLACE-ME"
base_url = "https://api.moonshot.cn/v1"
name = "Kimi"
models = ["moonshot-v1-8k", "moonshot-v1-32k", "moonshot-v1-128k"]

# --- OpenAI 兼容网关（vLLM / LiteLLM / 其他自建）---
[llm.providers.openai-compatible]
api_key = "sk-REPLACE-ME"
base_url = "https://your-openai-compatible-gateway.example.com/v1"
name = "OpenAI Compatible"
models = ["gpt-4o", "gpt-4o-mini"]

[server]
addr = "0.0.0.0:8787"
static_dir = "/usr/share/pointer-server/dist"
skills_dir = "/usr/share/pointer-server/skills"
# 浏览器实际访问地址（反代后的 HTTPS 域名，无尾斜杠）
public_url = "https://pointer.acme-corp.com"
# 数据持久化根目录（deb 默认 /var/lib/pointer-server）
app_data_dir = "/var/lib/pointer-server"

# standalone 模式下 [pointer] 段不参与 OAuth，可省略。
# 若保留，不影响 standalone 主流程：
# [pointer]
# api_base = "https://pointer-api.readflowai.com"
# oauth_client_secret = "unused-in-standalone"
```

环境变量可覆盖 TOML（优先级更高），见第 10 节。

---

## 7. LLM 供应商速查

| provider id | 供应商 | 默认 base_url | 默认 models |
|-------------|--------|---------------|-------------|
| `qwen` | 阿里云百炼 DashScope | `https://dashscope.aliyuncs.com/compatible-mode/v1` | `qwen3.5-plus`, `qwen3.5-turbo`, `qwen3.5-flash` |
| `deepseek` | DeepSeek 深度求索 | `https://api.deepseek.com/v1` | `deepseek-v4-pro`, `deepseek-v4-flash` |
| `glm` | 智谱 AI | `https://open.bigmodel.cn/api/paas/v4` | `glm-5`, `glm-5-flash` |
| `kimi` | 月之暗面 Moonshot | `https://api.moonshot.cn/v1` | `moonshot-v1-8k`, `moonshot-v1-32k`, `moonshot-v1-128k` |
| `openai-compatible` | 自建兼容网关 | 客户自定 | 客户自定 |

`active_provider` 决定默认对话模型来源；客户至少配置并填写**一个** Provider 的 `api_key`。若 `models` 不含内置默认模型名（如 `qwen3.5-plus`），启动注入后会自动把活跃模型改成该 Provider `models` 列表的第一项。

---

## 8. 登录与验收

### 8.1 启动检查

```bash
# License 状态
curl -s http://127.0.0.1:8787/api/license/status | jq .
# 期望：status = "valid"

# 服务健康（有 UI 时浏览器访问）
curl -s -o /dev/null -w "%{http_code}" http://127.0.0.1:8787/
# 期望：200
```

### 8.2 账号密码登录

**浏览器：** 打开 `public_url` → 欢迎页或设置 → 填写账号、密码、验证码登录（与 `[auth.local]` 一致）。

**生成 password_hmac：**

```bash
pointer-server --hash-password --secret 'REPLACE-WITH-LONG-RANDOM-SECRET' 'your-password'
```

**API：**

```bash
# captcha
curl -s http://127.0.0.1:8787/api/auth/local/captcha
# login
curl -X POST http://127.0.0.1:8787/api/auth/local/login \
  -H "Content-Type: application/json" \
  -d '{"username":"admin","password":"your-password","captchaId":"...","captcha":"ABCD"}' \
  -c /tmp/pointer-cookies.txt
```

### 8.3 功能验收清单

- [ ] License `valid`，`customerId` 正确
- [ ] 账号密码 + 验证码登录成功
- [ ] 发送对话，LLM 正常回复（验证 `active_provider` 的 `api_key`）
- [ ] 对话历史写入 `{app_data_dir}/conversations.db`
- [ ] （可选）Webhook 自动化触发
- [ ] （可选）IM 通道扫码注册后 WSS 立即连上（见第 9 节）

---

## 9. IM 通道（企微 / 飞书 / 钉钉）

Standalone server 支持 Webhook 与 WSS 长连接。扫码注册流程：

1. Web UI → 设置 → IM 通道 → 选择通道扫码
2. 注册成功后 server **自动**写入 `channels_config.json` 并重启 WSS monitor
3. 无需重启整个 pointer-server 进程

配置文件位置：`{app_data_dir}/channels_config.json`

各通道默认连接模式（扫码注册后自动设为 `websocket`）：

| 通道 | 供应商 | 注册方式 |
|------|--------|----------|
| 企微 WeCom | 腾讯企业微信 | QR 扫码 |
| 飞书 Feishu | 字节飞书 | QR 扫码 |
| 钉钉 DingTalk | 阿里钉钉 | QR 扫码 |
| 微信 Weixin | 腾讯 iLink | QR 扫码（桌面端能力，server 亦支持） |

---

## 10. 环境变量完整表

| 变量 | 说明 | standalone 典型值 |
|------|------|-------------------|
| `POINTER_DEPLOYMENT_MODE` | `platform` / `standalone` | `standalone` |
| `POINTER_SERVER_CONFIG` | 配置文件绝对路径 | `/etc/pointer-server/pointer-server.toml` |
| `POINTER_SERVER_ADMIN_USERNAME` | 管理员账号 | 与 TOML `[auth.local].username` 一致 |
| `POINTER_SERVER_ADMIN_PASSWORD_HMAC` | 密码 HMAC hex | 与 TOML `password_hmac` 一致 |
| `POINTER_SERVER_AUTH_HMAC_SECRET` | HMAC 密钥 | 与 TOML `hmac_secret` 一致 |
| `POINTER_LICENSE_KEY` | License 字符串 | 与 TOML `[license].key` 一致 |
| `POINTER_LICENSE_PUBLIC_KEY` | 仅 debug 可覆盖嵌入公钥；release 忽略 | 正式包勿依赖 |
| `POINTER_SERVER_ADDR` | 监听地址 | `0.0.0.0:8787` |
| `POINTER_SERVER_STATIC_DIR` | 静态资源目录 | `dist` 或 `/usr/share/pointer-server/dist`（TOML `[server].static_dir`） |
| `POINTER_SERVER_SKILLS_DIR` | 内置 Skills 源目录 | `skills` 或 `/usr/share/pointer-server/skills`（TOML `[server].skills_dir`） |
| `POINTER_SERVER_PUBLIC_URL` | 浏览器访问根 URL | `https://pointer.acme-corp.com` |
| `POINTER_APP_DATA_DIR` | 数据目录 | `/var/lib/pointer-server` |
| `POINTER_LLM_ACTIVE_PROVIDER` | 默认 LLM provider id | `qwen` |
| `POINTER_USAGE_REPORT_ENABLED` | 用量上报 | `false` |

TOML `[env]` 段可批量注入上述变量（见 `server/pointer-server.toml.example`）。

---

## 11. 生产环境：Nginx 反代示例

假设域名 `pointer.acme-corp.com`，后端监听 `127.0.0.1:8787`：

```nginx
server {
    listen 443 ssl http2;
    server_name pointer.acme-corp.com;

    ssl_certificate     /etc/letsencrypt/live/pointer.acme-corp.com/fullchain.pem;
    ssl_certificate_key /etc/letsencrypt/live/pointer.acme-corp.com/privkey.pem;

    client_max_body_size 64m;

    location / {
        proxy_pass http://127.0.0.1:8787;
        proxy_http_version 1.1;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;
        # SSE / 流式对话
        proxy_buffering off;
        proxy_cache off;
        proxy_read_timeout 3600s;
    }
}
```

`pointer-server.toml` 中 `public_url` 必须与浏览器地址一致：

```toml
public_url = "https://pointer.acme-corp.com"
```

---

## 12. 数据目录与备份

| 内容 | 路径 |
|------|------|
| 对话历史 | `{app_data_dir}/conversations.db` |
| Token 用量 | `{app_data_dir}/token_usage.db` |
| 任务板 | `{app_data_dir}/task_boards.db` |
| 工作项 | `{app_data_dir}/work_items.db` |
| IM 通道配置 | `{app_data_dir}/channels_config.json` |
| 会话日志 | `{app_data_dir}/logs/` |
| 附件媒体 | `{app_data_dir}/conversation-media/` |
| Memory | `{app_data_dir}/memories/` |

**备份策略：** 定期快照整个 `{app_data_dir}`；升级前务必备份。

---

## 13. License 热加载（不重启进程）

```bash
export POINTER_LICENSE_KEY="new_payload.new_signature"
curl -X POST http://127.0.0.1:8787/api/license/reload
curl -s http://127.0.0.1:8787/api/license/status | jq .
```

---

## 14. 端到端流程图

```text
Pointer 构建机                         Pointer 签发方                    客户生产机
─────────────                         ──────────────                    ──────────
npm run server:build
  → zip / deb 交付 ──────────────────────────────────────────────────→ 解压 / dpkg -i
                                                                        ./pointer-server --machine-id
                                              ←──── 机器 ID ────────────
npm run license-gen:build
./license-gen sign … ──→ License Key ────────────────────────────────→ 写入 pointer-server.toml
                                                                        配置 LLM api_key + auth.local
                                                                        systemctl start / ./start.sh
                                                                        账号密码登录 → 验收
```

---

## 15. 常见故障

| 现象 | 原因 | 处理 |
|------|------|------|
| `standalone mode requires a license key` | 未配置 License | 填写 `[license].key` 或 `POINTER_LICENSE_KEY` |
| `license bound to machine_id=…` | License 绑错机器 | 用当前 `--machine-id` 重新签发 |
| `license expired` | 过期 | 续签并 reload |
| `请先登录` / `local_login_required` | 未登录 | Web UI 账号密码 + 验证码，或 POST `/api/auth/local/login` |
| `invalid_credentials` | 账号/密码或 hmac 不匹配 | 用 `--hash-password` 重新生成 `password_hmac` |
| `invalid_captcha` | 验证码错误或过期 | 刷新验证码后重试 |
| 对话报 LLM 错误 | `api_key` 无效或未配置 | 检查对应 Provider 控制台 Key |
| IM 扫码成功但 WSS 不通 | 旧版本未 persist 配置 | 升级到含 registration persist 的版本；或手动保存通道配置并 `?restartMonitors=true` |
| macOS 上 `dpkg-deb failed` | deb 仅 Linux | 使用 `npm run server:build` 产出的 zip |

---

## 16. 相关文档

| 文档 | 读者 |
|------|------|
| [`../user/standalone-server.md`](../user/standalone-server.md) | 客户运维 |
| [`../developer/standalone-deployment.md`](../developer/standalone-deployment.md) | 开发实现 |
| [`../contributing/cross-platform-build.md`](../contributing/cross-platform-build.md) | 跨平台构建命令 |

[返回 internals 索引](README.md)
