# 独立部署 standalone-server

pointer-server 可以脱离官方平台独立部署，**不需要 Docker**，裸机编译运行。

---

## 快速开始

### 方式一：下载 .deb 安装（推荐 Linux）

```bash
# 安装
sudo dpkg -i pointer-server_0.1.0_amd64.deb

# 获取机器 ID（发给 License 签发方）
/usr/bin/pointer-server --machine-id

# 配置
sudo cp /etc/pointer-server/pointer-server.toml.example /etc/pointer-server/pointer-server.toml
sudo vi /etc/pointer-server/pointer-server.toml

# 启动
sudo systemctl enable --now pointer-server
```

### 方式二：从源码编译

```bash
# 构建前端
npm run build

# 编译服务端
cargo build -p pointer-server --release

# 产物：target/release/pointer-server
```

### 2. 获取机器 ID（新机器首次部署）

```bash
./pointer-server --machine-id
# 输出类似：5A372B48-8721-5807-9645-8E7A560F2518
```

把输出的 ID 发给 License 签发方。

### 3. 编辑配置

复制示例配置并修改：

```bash
cp server/pointer-server.toml.example ./pointer-server.toml
```

最小配置示例：

```toml
[deployment]
mode = "standalone"

[auth.local]
admin_token = "your-strong-secret"

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
app_data_dir = "/var/lib/pointer"
```

### 4. 启动

```bash
./target/release/pointer-server
```

浏览器打开 `http://localhost:8787` → 用 Admin Token 登录 → 开始对话。

---

## License 机制

### License 是什么

License 是 Ed25519 签名的 JSON 字符串，格式为 `base64(payload).base64(signature)`，包含：

- `customer_id`：客户标识
- `expires_at`：过期时间戳
- `features`：许可的功能（如 `chat`, `webhook`, `channels`）
- `max_seats`：最大用户数（可选）
- `machine_id`：绑定的机器 ID（可选，绑了就不能复制到其他机器）

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
  "currentMachineId": "5A372B48-8721-5807-9645-8E7A560F2518"
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
1. 下载 pointer-server 二进制
2. 运行获取机器 ID：
   ./pointer-server --machine-id
   → 5A372B48-...
    ──── 把 ID 发给签发方 ─────────→
                                    3. 签发绑定机器 ID 的 License：
                                       cargo run -p pointer-license-gen -- sign \\
                                         --private-key license.key \\
                                         --customer-id acme \\
                                         --expires 2027-12-31 \\
                                         --features chat,webhook \\
                                         --machine-id "5A372B48-..."
    ←──── 收到 License Key ─────────
4. 写入 pointer-server.toml：
   [license]
   key = "base64_payload.base64_signature"
5. 启动 → 成功
```

---

## 管理员登录

Standalone 模式下使用 **Admin Token** 代替官方 OAuth 登录：

```bash
curl -X POST http://localhost:8787/api/auth/local/login \
  -H "Content-Type: application/json" \
  -d '{"token": "your-strong-secret"}'
```

成功后服务端返回 `Set-Cookie` 头，后续请求自动带 Session。

`admin_token` 在 `pointer-server.toml` 的 `[auth.local]` 段配置。

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
| `POINTER_SERVER_ADMIN_TOKEN` | Admin Token | 无 |
| `POINTER_LICENSE_KEY` | License key 字符串 | 无 |
| `POINTER_LICENSE_PUBLIC_KEY` | 覆盖编译嵌入的公钥 | 无 |
| `POINTER_USAGE_REPORT_ENABLED` | 是否上报用量 | `false`（standalone） |
| `POINTER_SERVER_PUBLIC_URL` | 服务公网地址 | 自动推断 |
| `POINTER_APP_DATA_DIR` | 数据目录 | OS 默认 |
| `POINTER_SERVER_ALLOWED_USER_IDS` | 平台用户白名单 | 空 |

---

## 常见问题

### Q: 启动报 "standalone mode requires a license key"

未配置 License。在 `[license].key` 中设置有效的 License key。

### Q: 启动报 "license bound to machine_id=... but this machine is ..."

License 绑定了其他机器，需要申请当前机器的 License。运行 `./pointer-server --machine-id` 获取本机 ID。

### Q: 启动报 "请先登录 Pointer 账户"

Standalone 模式未配置 LLM Provider。检查 `[llm]` 配置段是否正确。
