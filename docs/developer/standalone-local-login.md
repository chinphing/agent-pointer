# Standalone 本地登录约定

独立部署（`deployment.mode = "standalone"`）使用 **单管理员账号密码**，不走平台 OAuth。

## 配置

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

## Web 行为

- `GET /api/auth/mode` → 前端在 standalone 下展示账号/密码/验证码表单（复用欢迎页、Composer、设置账户入口）。
- 登录成功写入 HttpOnly Cookie `pointer_web_session`（与平台 OAuth 同一会话机制）。
- 验证码为服务端内存 SVG，一次性，TTL 5 分钟。

桌面 Tauri 客户端不使用此登录路径。

详见 [standalone-deployment.md](standalone-deployment.md)、[../user/standalone-server.md](../user/standalone-server.md)。
