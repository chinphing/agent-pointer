# 官方包与本地构建

[English](../en/user/which-build.md) | 简体中文

开源仓库名为 **agent-pointer**。Pointer 只有一份源码，两种安装形态。

## 官方签名包

从 [官网下载页](https://pointer.readflowai.com/download) 下载、由维护者签名的安装包。

- 可选：登录 readflowai.com 账户
- 自动检查更新
- 登录后可上报用量、打开云主机、前往充值页
- 官方 `pointer-server` 安装包在独立部署时校验 License

## 本地构建

企业内部部署使用 [GitHub Releases](https://github.com/chinphing/agent-pointer/releases) 里发布的安装包。也可以从本仓库源码编译。

- 自己在设置里填写模型 Base URL 和 API Key
- 默认不绑定控制面，没有自动更新和用量上报
- 自建 `pointer-server` 不需要向签发方申请 License
- 云主机页可打开，未绑定控制面时不能购买

## 怎么选

| 你想要 | 用这个 |
| --- | --- |
| 开箱即用 | [官网下载](https://pointer.readflowai.com/download) |
| 企业内部部署 | [GitHub Releases](https://github.com/chinphing/agent-pointer/releases)，服务端见 [standalone-server.md](standalone-server.md) |

开发者如何用本机 `pointer.local.env` 绑定控制面或打本地包，见 [../zh-CN/deploy/editions.md](../zh-CN/deploy/editions.md)。
