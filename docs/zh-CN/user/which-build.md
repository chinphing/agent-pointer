# 官方包与本地构建

[English](../../en/user/which-build.md) | 简体中文

开源仓库名为 **agent-pointer**。Pointer 只有一份源码，两种安装形态。

## 官方签名包

从 [官网下载页](https://pointer.readflowai.com/download) 下载、由维护者签名的安装包。

- 可选：登录 readflowai.com 账户（**登录后会下发平台模型服务，无需自己配 Key**）
- 自动检查更新
- 登录后可上报用量、打开云主机、前往充值页
- 官方 `pointer-server` 安装包在独立部署时校验 License

## 本地构建

企业内部部署使用 [GitHub Releases](https://github.com/chinphing/agent-pointer/releases) 里发布的安装包。也可以从本仓库源码编译。

- 自己在设置里填写模型 Base URL 和 API Key
- 默认不绑定控制面，没有自动更新和用量上报
- 自建 `pointer-server` 不需要向签发方申请 License
- 没有云主机入口 —— 该分区只在**桌面端**且**非独立部署**时出现

## 怎么选

| 你想要 | 用这个 |
| --- | --- |
| 开箱即用 | [官网下载](https://pointer.readflowai.com/download) |
| 企业内部部署 | [GitHub Releases](https://github.com/chinphing/agent-pointer/releases)，服务端见 [standalone-server.md](standalone-server.md) |

## 自动更新

**官方签名包**带更新器，**本地构建没有** —— 这也是区分两种安装形态的一条硬线。

| 时机 | 行为 |
| --- | --- |
| 启动后约 30 秒 | 静默检查一次；有新版就在后台下载 |
| 之后每 6 小时 | 再静默检查一次 |
| 下载完成 | 提示 **立即更新 / 稍后 / 跳过此版本** |
| 想手动看 | 「关于」页的「检查更新」（**仅托管版桌面端**） |

- 检查与下载都是**后台静默**的，不打断你干活；只有「新版本已就绪」才提示
- 选「跳过此版本」会记住这个版本号，之后不再为它提示；出了更新的版本还会提示
- 手动检查时如果已经是最新，会显示「已是最新版本」
- **standalone / 本地构建没有更新器**：界面上不出现「检查更新」，也不会连任何更新服务

更新的地址由控制面（`POINTER_API_BASE`）提供。自建构建没有可连的更新源，所以本来也不该去更新。

开发者如何用本机 `pointer.local.env` 绑定控制面或打本地包，见 [../zh-CN/deploy/editions.md](../deploy/editions.md)。
