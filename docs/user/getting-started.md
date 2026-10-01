# 快速上手

[English](../en/user/getting-started.md) | 简体中文

先确认你用的是 [官方包还是本地构建](which-build.md)。

## 官方包

1. 从 [官网下载页](https://pointer.readflowai.com/download) 下载对应平台的安装包（Windows / macOS / Linux）
2. 打开 **设置**，填写模型 API Key（默认可走千问 DashScope 兼容接口）
3. 点击 **测试连接**
4. 发送一条消息，例如「你好」

**设置 → 账户** 登录官网后，才能使用云主机和充值。桌面 OAuth 成功后可能跳转官网。

## 本地构建 / 自建 Web

企业内部部署从 [GitHub Releases](https://github.com/chinphing/agent-pointer/releases) 获取安装包。从源码运行见仓库 [DEVELOPMENT.md](../../DEVELOPMENT.md)。部署 `pointer-server` 并用浏览器访问，见 [standalone-server.md](standalone-server.md)。

本地构建没有内置官网域名。模型地址和 Key 都在设置里自己填。

## 桌面端与 Web 端

| 入口 | 说明 |
|------|------|
| 桌面端 | Tauri 客户端，本地存储与完整能力（含 IM 通道、电脑操控等） |
| Web 端 | 浏览器访问已部署的 `pointer-server`，与桌面共用同一套对话逻辑 |

## 工具与确认

- 默认 **自动允许** 工具调用；可在设置中改为敏感工具需二次确认
- 联网搜索（`web_search`）需要有效的搜索/模型配置，按提供方计费

## 下一步

- [Skills 使用](skills.md)
- [IM 通道](im-channels.md)
- [云主机](cloud-host.md)（仅官方包）
