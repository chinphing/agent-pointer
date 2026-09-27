# Pointer

跨平台 AI 工作台：桌面端（Tauri 2 + Vue 3）与 Web 端（`pointer-server` + 同一套界面）。对话、工具、Skills 和 Agent 在 `crates/pointer-core`。

[English](README.md)

## 两种构建

| 构建 | 说明 |
| --- | --- |
| **官方**签名安装包 | 可选官网登录、自动更新、用量上报、云主机；官方 standalone 包校验 License |
| **社区**源码构建 | 同一套应用，自己配置模型。默认不连官方云，不自动更新，不强制 License |

见 [docs/user/editions.md](docs/user/editions.md)。

## 文档

| 读者 | 入口 |
| --- | --- |
| 用户 | [docs/user/](docs/user/README.md) |
| 开发者 | [docs/developer/](docs/developer/README.md) |
| 贡献者 | [CONTRIBUTING.md](CONTRIBUTING.md) · [DEVELOPMENT.md](DEVELOPMENT.md) |
| 安全 | [SECURITY.md](SECURITY.md) |
| 变更 | [CHANGELOG.md](CHANGELOG.md) |

完整索引：[docs/README.md](docs/README.md)。

## 开发

```bash
npm install
npm run tauri:dev
npm run server:dev
npm run web:dev
```

日常开发不要设置 `POINTER_EDITION`。打包与环境见 [DEVELOPMENT.md](DEVELOPMENT.md) 和 [docs/contributing/cross-platform-build.md](docs/contributing/cross-platform-build.md)。

## 许可证

Apache License 2.0。商标与官方域名见 [NOTICE](NOTICE)。
