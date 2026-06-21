# 云主机集成（pointer-app 桌面端）

桌面客户端通过 Pointer 平台 API 管理云主机，并以**独立 WebView 窗口**打开远程 pointer-server Web UI 进行对话。

## 桌面端能力

- **设置 → 云主机**：余额、实例列表、购买、续费、释放
- **打开**：签发 OAuth code，在新窗口加载 `console_url?code=&state=`
- **切换到云窗口 / 关闭云窗口**：回到本地主窗口继续本地对话

需先 **设置 → 平台账户** 完成 OAuth 登录。

## 云实例环境约定（自行部署）

在 Windows ECS 上部署 pointer-server + Vue 静态资源，并配置：

| 项 | 说明 |
|----|------|
| 监听 | `POINTER_SERVER_ADDR=0.0.0.0:${AGENT_PORT}`（与平台 ECS profile 一致） |
| 健康检查 | `GET /api/health` 返回 2xx；平台配置 `AGENT_HEALTH_PATH=/api/health` |
| `OPENPOINTER_API_BASE` | 平台 API 根地址 |
| `OPENPOINTER_OAUTH_CLIENT_SECRET` | 与平台一致的换码密钥 |

数据、日志与会话存储默认与桌面客户端相同（`{OS 用户数据目录}/PointerApp`，debug 构建为 `PointerAppDev`；日志在 `…/PointerApp/logs/`）；无需在 `pointer-server.toml` 里单独配 `app_data_dir`，除非要指向自定义路径。

Windows 部署可在 exe 同目录放 `pointer-server.toml`（见 `server/pointer-server.toml.example`），NSSM 只需注册 exe，不必再写 `AppEnvironmentExtra`。

用户从桌面「打开」时，浏览器/WebView 访问带 `code` 的 URL；pointer-server 在 SPA fallback 中服务端换码（`POST /auth/oauth/exchange-code`），注入 LLM 凭证后 302 到 `/`。

## 认证链路

```text
桌面客户端 Bearer → 平台 API（购买 / oauth-code）
WebView → 云实例 ?code= → 云 pointer-server 换码 → partner JWT + LLM keys
对话 → 云 pointer-server 使用 partner 会话上报 token usage
```

## 联调清单

1. 桌面平台登录成功
2. 云实例 `/api/health` 可达
3. 客户端购买后 Worker 探活 `app_ready`
4. 「打开」换码成功，云窗口可发消息
5. 关闭云窗口后本地主窗口正常
6. 续费 / 释放 API 行为符合预期

## 第二期（未实现）

- 到期 **StopInstance** 停机保数据，续开 **StartInstance**
- 主界面 **Tab** 切换本地 / 云主机（替代多窗口）
