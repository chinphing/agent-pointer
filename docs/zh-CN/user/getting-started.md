# 快速上手

[English](../../en/user/getting-started.md) | 简体中文

先确认你用的是 [官方包还是本地构建](which-build.md)。

## 官方包

1. 从 [官网下载页](https://pointer.readflowai.com/download) 下载对应平台的安装包（Windows / macOS / Linux）
2. **配置模型**，二选一：
   - **登录平台账号**（左下角「**账户**」菜单 →「**登录**」）：登录后平台会下发模型服务，直接就能用
   - **自己配**：在 **设置 → 模型配置 → 自定义服务** 填 Base URL 和 API Key，点 **测试连接**。详见 [配置模型服务](model-providers.md)
3. 发送一条消息，例如「你好」

登录平台账号后还能使用云主机和充值。桌面 OAuth 成功后可能跳转官网。

## 本地构建 / 自建 Web

企业内部部署从 [GitHub Releases](https://github.com/chinphing/agent-pointer/releases) 获取安装包。从源码运行见仓库 [DEVELOPMENT.md](../../../DEVELOPMENT.md)。部署 `pointer-server` 并用浏览器访问，见 [standalone-server.md](standalone-server.md)。

本地构建没有内置官网域名。模型地址和 Key 都在设置里自己填。

## 桌面端与 Web 端

| 入口 | 说明 |
|------|------|
| 桌面端 | Tauri 客户端，本地存储与完整能力（含 IM 通道、电脑操控等） |
| Web 端 | 浏览器访问已部署的 `pointer-server`，与桌面共用同一套对话逻辑 |

## 工具与确认

- 默认 **自动允许** 工具调用；可在设置中改为敏感工具需二次确认
- 联网搜索（`web_search`）需要有效的搜索/模型配置，按提供方计费

## 键盘快捷键

输入框里这几个键值得记住：

| 按键 | 行为 |
| --- | --- |
| `Enter` | 发送。正在生成时**入队**，等这一轮结束再发 |
| `Shift+Enter` | 换行 |
| `⌘/Ctrl+Enter` | **软取消当前回合并立即发送** |
| `Enter`（输入框为空、且有排队消息） | 立即发送队首 |
| `⌘/Ctrl+F` | 页内搜索。焦点在工作区面板时，优先给文件树或文件预览 |
| `Esc` | 关闭搜索 / 重命名 / 弹窗 / 图片预览 |

`⌘/Ctrl+Enter` 是最容易记错的一个：它不是「换行」，也不是普通的「发送」，而是**先停掉正在跑的这一轮**（只停同步的这一轮，后台任务继续），再把你当前输入的内容立刻发出去 —— 相当于「打断 + 抢话」，和 Cursor 里手感一致。

## 下一步

- [Skills 使用](skills.md)
- [IM 通道](im-channels.md)
