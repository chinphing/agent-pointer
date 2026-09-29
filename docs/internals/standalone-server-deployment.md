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

- **官方包必须**有效 License（Ed25519 验签，公钥编译在 `crates/pointer-core/license.pub`）；非 official 自建包不强制 —— 未配置时以 `notConfigured` 运行，许可功能关闭
- **必须**在 Web 设置 → 模型配置 添加服务并填写 API Key（不要写在 TOML）
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

以下模板包含 standalone 所需的**全部默认段**。模型与 API Key 不在此文件，登录后于 **设置 → 模型配置** 填写。

配置文件查找顺序：

1. 环境变量 `POINTER_SERVER_CONFIG` 指向的绝对路径
2. 可执行文件同目录的 `pointer-server.toml`

```toml
# =============================================================================
# pointer-server standalone 生产配置模板
# 复制为 pointer-server.toml 后修改 auth.local、license.key、public_url
# 模型与 API Key 在登录后于设置 → 模型配置 填写
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

[server]
addr = "0.0.0.0:8787"
static_dir = "/usr/share/pointer-server/dist"
skills_dir = "/usr/share/pointer-server/skills"
# 浏览器实际访问地址（反代后的 HTTPS 域名，无尾斜杠）
public_url = "https://pointer.acme-corp.com"
# 数据持久化根目录（deb 默认 /var/lib/pointer-server）
app_data_dir = "/var/lib/pointer-server"
# Web 品牌文案（可选；详见 docs/ui/web-branding-welcome-elapsed.md）
# page_title = "Acme · AI 助手"
# composer_placeholder = "有什么可以帮你？"
# welcome_tip_title = "我是财务报销助手"
# welcome_tip_body = "您提交附件后我会自动帮你填报销单，预计 10–30 分钟，期间您可以离开，完成任务后您回来确认信息即可。"
# turn_elapsed_active = "报销单填写中"
# turn_elapsed_done = "报销单已填写"
# brand_name = "财务助手"
# brand_icon = "/branding/logo.png"   # 左上角与左下角共用
# desktop_snapshot_enabled = false    # 未设则自动探测显示器
# SSE 首帧 padding（穿透缓冲型反代时按需开启）
# sse_padding_enabled = false
# sse_padding_bytes = 10240

# standalone 模式下 [pointer] 段不参与 OAuth，可省略。
# 若保留，不影响 standalone 主流程：
# [pointer]
# api_base = "https://pointer-api.readflowai.com"
# oauth_client_secret = "unused-in-standalone"
```

环境变量可覆盖 TOML（优先级更高），见第 10 节。

---

## 7. 模型配置

登录后打开 **设置 → 模型配置**，添加自定义服务（地址、模型名单、API Key、上下文、思考强度、`extra_body` 等），与桌面客户端自定义服务相同。不要在 TOML 或环境变量里配置模型；旧 `[llm]` / `POINTER_LLM_ACTIVE_PROVIDER` 已废弃。

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
- [ ] 发送对话，LLM 正常回复（先在设置 → 模型配置添加服务并填写 API Key）
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
| `POINTER_SERVER_PAGE_TITLE` | 浏览器标签页标题 | `Pointer · AI 工作台` |
| `POINTER_SERVER_COMPOSER_PLACEHOLDER` | 输入框占位文案 | `告诉我你想做什么` |
| `POINTER_SERVER_WELCOME_TIP_TITLE` | 全新空会话欢迎 tip 标题（可选） | （不展示 tip） |
| `POINTER_SERVER_WELCOME_TIP_BODY` | 全新空会话欢迎 tip 正文（可选） | （不展示 tip） |
| `POINTER_SERVER_TURN_ELAPSED_ACTIVE` | 进行中回合耗时前缀（可选） | `工作` |
| `POINTER_SERVER_TURN_ELAPSED_DONE` | 已结束回合耗时前缀（可选） | `工作` |
| `POINTER_SERVER_BRAND_NAME` | 侧栏/顶栏产品名（可选） | `Pointer` |
| `POINTER_SERVER_BRAND_ICON` | 左上角与左下角共用 logo（可选） | `/app-icon.png` |
| `POINTER_SERVER_DESKTOP_SNAPSHOT_ENABLED` | 桌面截图按钮（可选；未设则自动探测显示器） | 自动 |
| `POINTER_SERVER_SSE_PADDING_ENABLED` | SSE 首帧 padding | `false` |
| `POINTER_SERVER_SSE_PADDING_BYTES` | padding 字节数 | `10240` |
| `POINTER_USAGE_REPORT_ENABLED` | 用量上报 | `false` |
| `POINTER_SERVER_FORBID_SESSION_USER_ID_IN_TERMINAL` | Agent `terminal` 禁止 command/stdin 含 `SESSION_USER_ID`（TOML `[server].forbid_session_user_id_in_terminal`） | 默认关闭；需要时 `true` |
| `POINTER_SERVER_CORS_ORIGINS` | 浏览器 CORS Origin 列表（TOML `[server].cors_origins`；`*` 镜像任意来源） | 默认关闭（仅同源）；前后端分离或 `web:dev` 直连 API 时再开 |

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
        # SSE / 流式对话（聊天事件流路径为 /api/chat/*/stream，不是 /sse）
        proxy_buffering off;
        proxy_cache off;
        proxy_read_timeout 3600s;
        proxy_send_timeout 3600s;
    }
}
```

`pointer-server` 的 SSE 响应会带 `X-Accel-Buffering: no`，在未写 `proxy_buffering off` 的 location 上也可让 Nginx 对本响应关闭缓冲；**`proxy_read_timeout` 仍须在反代侧配置**（应用无法改写网关超时）。直连 `pointer-server`（无反代）无需这些项。

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
