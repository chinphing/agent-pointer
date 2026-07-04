# 快速上手

## 安装

- **桌面端**：从官网下载对应平台的安装包（Windows / macOS / Linux）。
- **Web 端**：由管理员部署 `pointer-server` 后通过浏览器访问（见 [`../developer/cloud-host-integration.md`](../developer/cloud-host-integration.md) 自行部署章节）。

## 配置模型

1. 打开 **设置**
2. 填入 **DashScope API Key**（阿里云百炼控制台获取）
3. 点击 **测试连接**
4. 发送简单消息验证，例如 `你好`

默认 Provider 为千问 `qwen-plus`，Base URL：`https://dashscope.aliyuncs.com/compatible-mode/v1`。

## 桌面端与 Web 端

| 入口 | 说明 |
|------|------|
| 桌面端 | Tauri 客户端，本地存储与完整能力（含 IM 通道、电脑操控等） |
| Web 端 | 浏览器访问已部署的 `pointer-server`，与桌面共用同一套对话逻辑 |

Release 安装包已内置官网与 API 地址，一般无需配置环境变量。本地开发见仓库根目录 [`DEVELOPMENT.md`](../../DEVELOPMENT.md)。

## 平台账户（可选）

**设置 → 平台账户** 完成 OAuth 登录后，可使用云主机、官网同步能力等。桌面 OAuth 成功后可能跳转官网并显示一次性提示。

## 工具与确认

- 默认 **自动允许** 工具调用；可在设置中改为敏感工具需二次确认。
- 联网搜索（`web_search`）需有效 DashScope Key，按模型与搜索用量计费。

## 下一步

- [Skills 使用](skills.md)
- [IM 通道](im-channels.md)
- [云主机](cloud-host.md)
