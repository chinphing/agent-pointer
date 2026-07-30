# 独立部署 standalone-server

pointer-server 可脱离官方平台独立部署。交付物为**安装包**（如 Linux `.deb`），无需 Docker，也无需源码。

---

## 快速开始

### 1. 安装

以 Linux `.deb` 为例（包名以实际交付为准）：

```bash
sudo dpkg -i pointer-server_0.1.0_amd64.deb
```

安装后常用路径：

| 内容 | 路径 |
|------|------|
| 可执行文件 | `/usr/bin/pointer-server` |
| 配置目录 | `/etc/pointer-server/` |
| 配置示例 | `/etc/pointer-server/pointer-server.toml.example` |
| 正式配置 | `/etc/pointer-server/pointer-server.toml` |
| systemd 服务 | `pointer-server` |

### 2. 获取机器绑定信息（首次部署）

```bash
/usr/bin/pointer-server --machine-id-json   # 推荐：完整 JSON（含漂移锚点）
# 或
/usr/bin/pointer-server --machine-id        # 仅主 token（fp1:…）
```

把输出发给 License 签发方。v2 指纹在 OS 重装后仍可通过 board / cloud 锚点验证。

### 3. 编辑配置

```bash
sudo cp /etc/pointer-server/pointer-server.toml.example /etc/pointer-server/pointer-server.toml
sudo vi /etc/pointer-server/pointer-server.toml
```

最小配置示例：

```toml
[deployment]
mode = "standalone"

[auth.local]
username = "admin"
hmac_secret = "replace-with-long-random-secret"
# Generate with: pointer-server --hash-password --secret '<hmac_secret>' '<password>'
password_hmac = "..."

[llm]
active_provider = "qwen"

[llm.providers.qwen]
api_key = "sk-..."
base_url = "https://dashscope.aliyuncs.com/compatible-mode/v1"
name = "千问"
models = ["qwen3.5-plus"]

[license]
key = "base64_payload.base64_signature"

[usage]
report_enabled = false

[server]
addr = "0.0.0.0:8787"
public_url = "https://pointer.example.com"
# Browser tab title (optional; default Pointer · AI 工作台)
# page_title = "Acme · AI 助手"
# Composer placeholder (optional; default 告诉我你想做什么)
# composer_placeholder = "有什么可以帮你？"
app_data_dir = "/var/lib/pointer"
```

生成登录密码摘要（写入 `[auth.local].password_hmac`）：

```bash
/usr/bin/pointer-server --hash-password --secret 'replace-with-long-random-secret' 'your-password'
# 输出：password_hmac = "...."
```

### 4. 启动

```bash
sudo systemctl enable --now pointer-server
sudo systemctl status pointer-server
```

浏览器打开配置的 `public_url`（本机可先用 `http://localhost:8787`）→ 用账号、密码、验证码登录 → 开始对话。

---

## License 机制

### License 是什么

License 是 Ed25519 签名的 JSON 字符串，格式为 `base64(payload).base64(signature)`，包含：

- `customer_id`：客户标识
- `expires_at`：过期时间戳
- `features`：许可的功能（如 `chat`, `webhook`, `channels`）
- `max_seats`：最大用户数（可选）
- `machine_id`：绑定的机器 token（`fp1:…` 或 legacy os id，可选）
- `machine_board_fp` / `machine_cloud_fp`：漂移锚点（v2，OS 重装后仍可验证）

### License 验证流程

```
启动时 → 读取 license key → 解包 payload → Ed25519 验签
  → 检查过期 → 检查机器绑定 → 通过 → 正常运行
```

### 如何判断 License 状态

```bash
curl http://localhost:8787/api/license/status
```

返回示例：

```json
{
  "status": "valid",
  "customerId": "acme",
  "expiresAt": 1893455999,
  "features": ["chat", "webhook"],
  "machineBound": true,
  "currentMachineId": "fp1:a1b2c3..."
}
```

`status` 可能的值：

| 值 | 说明 |
|----|------|
| `valid` | 正常 |
| `expired` | 已过期 |
| `notConfigured` | 未配置 License |
| `invalid` | 签名无效 |
| `machineMismatch` | 机器不匹配 |

### 热加载 License（不重启）

```bash
# 设置新 key
export POINTER_LICENSE_KEY="new_payload.new_signature"
# 热加载
curl -X POST http://localhost:8787/api/license/reload
```

---

## 新机器部署完整流程

```
客户方                              签发方（管理员）
──────                              ─────────────────
1. 安装交付的 pointer-server 安装包
2. 获取绑定信息：
   /usr/bin/pointer-server --machine-id-json > identity.json
    ──── 把 identity.json 发给签发方 ─→
                                    3. 签发绑定 License
    ←──── 收到 License Key ─────────
4. 写入 /etc/pointer-server/pointer-server.toml：
   [license]
   key = "base64_payload.base64_signature"
5. systemctl enable --now pointer-server → 成功
```

---

## 管理员登录

Standalone **不走**官网云电脑 `code/state` 换码。支持：

1. **第三方 SSO**：门户签发短时票后打开 `https://{public_url}/?sso=<ticket>`（配置见开发者文档 [standalone-local-login.md](../developer/standalone-local-login.md)）。
2. **账号密码 + 图形验证码**（运维备用）：

```bash
# 1) 取验证码
curl -s http://localhost:8787/api/auth/local/captcha
# → {"captchaId":"...","imageSvg":"<svg>...</svg>"}

# 2) 登录（验证码看 SVG）
curl -X POST http://localhost:8787/api/auth/local/login \
  -H "Content-Type: application/json" \
  -c /tmp/pointer-cookies.txt \
  -d '{"username":"admin","password":"your-password","captchaId":"...","captcha":"ABCD"}'
```

成功后服务端返回 `Set-Cookie`（`pointer_web_session`），后续请求自动带 Session。

配置：`[auth.local]`（密码）与可选 `[auth.local.sso]`（第三方跳转）。
旧版 `admin_token` 已废弃并忽略。

---

## LLM Provider 配置

支持以下 Provider：

| Provider ID | 默认 API Base |
|-------------|--------------|
| `qwen` | `https://dashscope.aliyuncs.com/compatible-mode/v1` |
| `deepseek` | `https://api.deepseek.com/v1` |
| `glm` | `https://open.bigmodel.cn/api/paas/v4` |
| `kimi` | `https://api.moonshot.cn/v1` |
| `openai-compatible` | 自定义 |

```toml
[llm]
active_provider = "qwen"

[llm.providers.qwen]
api_key = "sk-..."
base_url = "https://dashscope.aliyuncs.com/compatible-mode/v1"
name = "千问"
models = ["qwen3.5-plus", "qwen3.5-turbo"]
```

`models` 第一项会在当前活跃模型不在列表内时作为默认对话模型（本地/自建模型名也适用）。

---

## 数据目录

通过 `[server].app_data_dir` 指定所有运行时数据的根目录：

| 内容 | 路径 |
|------|------|
| 对话历史 | `{app_data_dir}/conversations.db` |
| Token 用量 | `{app_data_dir}/token_usage.db` |
| 任务板 | `{app_data_dir}/task_boards.db` |
| 工作项 | `{app_data_dir}/work_items.db` |
| 会话日志 | `{app_data_dir}/logs/` |
| 附件媒体 | `{app_data_dir}/conversation-media/` |
| Memory | `{app_data_dir}/memories/` |

---

## 环境变量

| 变量 | 说明 | 默认值 |
|------|------|--------|
| `POINTER_DEPLOYMENT_MODE` | `platform` / `standalone` | `platform` |
| `POINTER_SERVER_ADMIN_USERNAME` | 管理员账号 | 无 |
| `POINTER_SERVER_ADMIN_PASSWORD_HMAC` | 密码 HMAC-SHA256 hex | 无 |
| `POINTER_SERVER_AUTH_HMAC_SECRET` | 计算 password_hmac 的密钥 | 无 |
| `POINTER_LICENSE_KEY` | License key 字符串 | 无 |
| `POINTER_LICENSE_PUBLIC_KEY` | **仅 debug 二进制**可覆盖编译嵌入的公钥；正式 release 包忽略此变量 | 无 |
| `POINTER_USAGE_REPORT_ENABLED` | 是否上报用量 | `false`（standalone） |
| `POINTER_SERVER_PUBLIC_URL` | 服务公网地址 | 自动推断 |
| `POINTER_SERVER_PAGE_TITLE` | 浏览器标签页标题（`index.html` `<title>`） | `Pointer · AI 工作台` |
| `POINTER_SERVER_COMPOSER_PLACEHOLDER` | 输入框默认提示文案（写入 `pointer-composer-placeholder` meta） | `告诉我你想做什么` |
| `POINTER_APP_DATA_DIR` | 数据目录 | OS 默认 |
| `POINTER_SERVER_STATIC_DIR` | Web UI `dist/` 目录 | 自动探测（含 deb 的 `/usr/share/pointer-server/dist`） |
| `POINTER_SERVER_SKILLS_DIR` | 内置 Skills 源目录 | 自动探测（含 deb 的 `/usr/share/pointer-server/skills`） |
| `POINTER_SERVER_ALLOWED_USER_IDS` | 平台用户白名单 | 空 |

---

## 常见问题

### Q: 启动报 "standalone mode requires a license key"

未配置 License。在 `[license].key` 中设置有效的 License key。

### Q: 启动报 "license bound to machine_id=... but this machine is ..."

License 绑定了其他机器，需要申请当前机器的 License。运行 `/usr/bin/pointer-server --machine-id-json` 获取完整绑定信息。

### Q: 提示「请先登录」

Standalone 下未登录 Web 会话。打开页面用账号密码 + 验证码登录；确认 `[auth.local]` 已配置且 `password_hmac` 与 `hmac_secret` 匹配。
