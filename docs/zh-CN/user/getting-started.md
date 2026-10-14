# 快速上手

[English](../../en/user/getting-started.md) | 简体中文

## 现在就用

1. 打开 [官网下载页](https://pointer.readflowai.com/download)，安装，打开 Pointer。
2. 左下角 **账户** → **登录**。登录后平台会下发模型。
3. 发一句「你好」。

还没有平台账号时，到 **设置 → 模型配置 → 自定义服务** 填 Base URL 和 API Key，点 **测试连接**。详见 [配置模型服务](model-providers.md)。

登录之后还可以用云主机和充值。桌面登录成功后可能跳到官网，回到窗口即可。

## 不是从官网安装时

[官方包和本地构建的差别](which-build.md) 可以之后再看。公司内部分发的安装包在 [GitHub Releases](https://github.com/chinphing/agent-pointer/releases)。在仓库里用脚本打开桌面程序，不需要先编译，见 [本地启动](../deploy/quick-start.md)。自己改代码用 npm，见 [开发第一步](../../../DEVELOPMENT.md)。用浏览器访问一台已部署的服务端，见 [standalone-server.md](standalone-server.md)。

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
